//! `specify-core mcp`: one conversation per protocol era (2026-07-28 and 2025-11-25) over
//! stdio-shaped in-memory pipes, each tool's structured result checked against `serve`'s
//! reply to the same request, and the refusals: paths outside `--allow`, overwriting, unknown
//! and stale handles, unsupported versions, and the HTTP transport's token, Host and Origin.

mod common;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use serde_json::{Map, Value, json};
use specify_core::mcp::{self, Server};
use specify_core::serve;

const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const META_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";

fn part(stem: &str) -> PathBuf {
    common::fixtures()
        .join("parts")
        .join(format!("{stem}.step"))
}

fn revision(name: &str) -> PathBuf {
    common::fixtures().join("revisions").join(name)
}

/// A fresh, empty, canonical temporary directory for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("specify-mcp-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

// ----- an in-memory stdio pipe -----

/// The reading end of an in-memory pipe: lines sent on the channel, as a byte stream.
struct PipeIn {
    lines: Receiver<String>,
    pending: Vec<u8>,
}

impl Read for PipeIn {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pending.is_empty() {
            match self.lines.recv() {
                Ok(line) => self.pending = line.into_bytes(),
                Err(_) => return Ok(0), // the client closed stdin
            }
        }
        let n = buf.len().min(self.pending.len());
        buf[..n].copy_from_slice(&self.pending[..n]);
        self.pending.drain(..n);
        Ok(n)
    }
}

/// The writing end: every byte written goes to the client.
struct PipeOut(Sender<Vec<u8>>);

impl Write for PipeOut {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.send(buf.to_vec()).map_err(std::io::Error::other)?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// An MCP client talking to [`mcp::serve_stdio`] on its own thread, one line each way.
struct Stdio {
    to_server: Option<Sender<String>>,
    from_server: Receiver<Vec<u8>>,
    buffer: Vec<u8>,
    server: Option<thread::JoinHandle<()>>,
    next: i64,
    era: &'static str,
}

impl Stdio {
    fn start(allow: &[PathBuf], era: &'static str) -> Self {
        let (to_server, lines) = channel::<String>();
        let (out, from_server) = channel::<Vec<u8>>();
        let allow = allow.to_vec();
        let server = thread::spawn(move || {
            let mut server = Server::new(&allow).unwrap();
            let input = BufReader::new(PipeIn {
                lines,
                pending: Vec::new(),
            });
            mcp::serve_stdio(input, PipeOut(out), &mut server).unwrap();
        });
        Stdio {
            to_server: Some(to_server),
            from_server,
            buffer: Vec::new(),
            server: Some(server),
            next: 1,
            era,
        }
    }

    fn send(&mut self, message: &Value) {
        let line = format!("{message}\n");
        self.to_server.as_ref().unwrap().send(line).unwrap();
    }

    /// One line from the server, which must be a JSON-RPC message.
    fn receive(&mut self) -> Value {
        loop {
            if let Some(end) = self.buffer.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = self.buffer.drain(..=end).collect();
                let text = String::from_utf8(line).unwrap();
                return serde_json::from_str(&text)
                    .unwrap_or_else(|e| panic!("not a message: {text:?}: {e}"));
            }
            let chunk = self.from_server.recv().expect("the server answers");
            self.buffer.extend(chunk);
        }
    }

    /// A request in this client's era, and its response.
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        let mut params = params.as_object().cloned().unwrap_or_default();
        if self.era == mcp::MODERN {
            params.insert(
                "_meta".into(),
                json!({META_VERSION: mcp::MODERN, META_CAPABILITIES: {},
                       "io.modelcontextprotocol/clientInfo": {"name": "test", "version": "0"}}),
            );
        }
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let response = self.receive();
        assert_eq!(response["id"], json!(id), "{response}");
        assert_eq!(response["jsonrpc"], json!("2.0"));
        response
    }

    fn result(&mut self, method: &str, params: Value) -> Value {
        let response = self.request(method, params);
        assert!(response.get("error").is_none(), "{method}: {response}");
        let result = response["result"].clone();
        if self.era == mcp::MODERN {
            assert_eq!(result["resultType"], json!("complete"), "{result}");
        } else {
            assert!(
                result.get("resultType").is_none(),
                "legacy results carry no resultType"
            );
        }
        result
    }

    /// A tool's structured result, which must not be an error.
    fn call(&mut self, name: &str, arguments: Value) -> Value {
        let result = self.result("tools/call", json!({"name": name, "arguments": arguments}));
        assert_eq!(result["isError"], json!(false), "{name}: {result}");
        // The same JSON as text, for clients that read only content.
        let text: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text, result["structuredContent"]);
        result["structuredContent"].clone()
    }

    /// A tool's refusal: its `error`.
    fn refusal(&mut self, name: &str, arguments: Value) -> Value {
        let result = self.result("tools/call", json!({"name": name, "arguments": arguments}));
        assert_eq!(result["isError"], json!(true), "{name}: {result}");
        result["structuredContent"]["error"].clone()
    }

    /// Closes stdin: the server exits cleanly and wrote nothing more.
    fn finish(mut self) {
        drop(self.to_server.take());
        self.server.take().unwrap().join().unwrap();
        let rest: Vec<u8> = self.from_server.try_iter().flatten().collect();
        assert!(
            self.buffer.is_empty() && rest.is_empty(),
            "nothing but responses on stdout"
        );
    }
}

/// `serve`'s result for a request (which must succeed).
fn serve_ok(server: &mut serve::Server, request: Value) -> Value {
    let reply = server.reply(&request);
    assert_eq!(reply["ok"], json!(true), "{reply}");
    reply["result"].clone()
}

/// Every field of `serve`'s question is the MCP question's, which adds presentation and state.
fn same_questions(mcp: &Value, serve: &Value) {
    let mcp = mcp.as_array().unwrap();
    let serve = serve.as_array().unwrap();
    assert_eq!(mcp.len(), serve.len());
    for (m, s) in mcp.iter().zip(serve) {
        for (key, value) in s.as_object().unwrap() {
            assert_eq!(&m[key], value, "{}: {key}", s["id"]);
        }
        assert!(["open", "answered", "optional"].contains(&m["state"].as_str().unwrap()));
    }
}

/// A write report without where it was written.
fn report_without_output(report: &Value) -> Value {
    let mut r = report.as_object().unwrap().clone();
    r.remove("output");
    Value::Object(r)
}

/// The whole conversation in one era, each step checked against `serve`.
fn conversation(era: &'static str) {
    let dir = scratch(&format!("conversation-{era}"));
    let allow = [common::fixtures(), dir.clone()];
    let mut client = Stdio::start(&allow, era);
    let mut serve = serve::Server::default();
    let plate = part("plate");

    if era == mcp::MODERN {
        let discovered = client.result("server/discover", json!({}));
        assert_eq!(
            discovered["supportedVersions"],
            json!([mcp::MODERN, mcp::LEGACY])
        );
        assert!(discovered["capabilities"]["tools"].is_object());
        assert!(discovered["ttlMs"].as_u64().is_some());
        assert_eq!(
            discovered["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
            json!("specify-core")
        );
    } else {
        let init = client.result(
            "initialize",
            json!({"protocolVersion": mcp::LEGACY, "capabilities": {},
                   "clientInfo": {"name": "test", "version": "0"}}),
        );
        assert_eq!(init["protocolVersion"], json!(mcp::LEGACY));
        assert!(init["capabilities"]["resources"].is_object());
        client.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        assert_eq!(
            client.result("ping", json!({})).as_object().unwrap().len(),
            1
        ); // _meta only
    }

    let tools = client.result("tools/list", json!({}));
    let names: Vec<&str> = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            assert!(t["inputSchema"]["type"] == "object" && t["outputSchema"]["type"] == "object");
            t["name"].as_str().unwrap()
        })
        .collect();
    assert_eq!(
        names,
        [
            "list_parts",
            "open",
            "questions",
            "answer",
            "review",
            "characteristics",
            "read_pmi",
            "write_pmi",
            "export_answers",
            "diff"
        ]
    );

    // list_parts == serve's parts.
    let listed = client.call("list_parts", json!({"path": plate}));
    let parts = serve_ok(&mut serve, json!({"op": "parts", "step": plate}));
    assert_eq!(listed["parts"], parts["parts"]);

    // open: a handle; its analysis and mesh resources == serve's analyse.
    let opened = client.call("open", json!({"path": plate}));
    let doc = opened["doc"].as_str().unwrap().to_string();
    let analysed = serve_ok(&mut serve, json!({"op": "analyse", "step": plate}));
    let analysis = analysed["analysis"].clone();
    assert!(doc.starts_with(analysis["binding"]["source_sha256"].as_str().unwrap()));
    assert_eq!(opened["faces"], analysis["faces"].as_array().unwrap().len());
    let read = |client: &mut Stdio, kind: &str| -> Value {
        let uri = format!("specify://doc/{doc}/{kind}");
        let read = client.result("resources/read", json!({"uri": uri}));
        assert_eq!(read["contents"][0]["uri"], json!(uri));
        serde_json::from_str(read["contents"][0]["text"].as_str().unwrap()).unwrap()
    };
    assert_eq!(read(&mut client, "analysis"), analysis);
    assert_eq!(read(&mut client, "mesh"), analysed["mesh"]);
    let listed = client.result("resources/list", json!({}));
    assert_eq!(listed["resources"].as_array().unwrap().len(), 2);
    let templates = client.result("resources/templates/list", json!({}));
    assert_eq!(
        templates["resourceTemplates"][0]["uriTemplate"],
        json!("specify://doc/{doc}/analysis")
    );

    // questions == serve's questions; open questions as open returned them.
    let asked = client.call("questions", json!({"doc": doc, "state": "all"}));
    let served = serve_ok(
        &mut serve,
        json!({"op": "questions", "analysis": analysis, "answers": {}}),
    );
    same_questions(&asked["questions"], &served["questions"]);
    assert_eq!(asked["open"], served["open"]);
    assert_eq!(opened["open_questions"], served["open"]);
    let only_open = client.call("questions", json!({"doc": doc}));
    assert_eq!(
        only_open["questions"].as_array().unwrap().len(),
        served["open"].as_array().unwrap().len()
    );

    // answer, by free text: the tapped hole's "M5", read as serve's interpret reads it.
    let hole = asked["questions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|q| {
            q["id"].as_str().unwrap().starts_with("hole.function:") && q["default"] == "tapped:M5"
        })
        .expect("the plate's tapped hole")["id"]
        .as_str()
        .unwrap()
        .to_string();
    let answered = client.call(
        "answer",
        json!({"doc": doc, "set": {&hole: "M5", "part.material": "6082-T6"}}),
    );
    let interpreted = serve_ok(
        &mut serve,
        json!({
            "op": "interpret", "analysis": analysis, "answers": {}, "question": hole, "text": "M5",
        }),
    );
    assert_eq!(answered["set"][0]["value"], interpreted["value"]);
    assert_eq!(answered["set"][0]["interpreted_from"], json!("M5"));
    assert_eq!(answered["set"][0]["state"], json!("answered"));
    assert_eq!(answered["set"][1]["value"], json!("6082-T6"));
    let answers = json!({&hole: interpreted["value"], "part.material": "6082-T6"});

    // A refused answer names its question and changes nothing.
    let refused = client.refusal("answer", json!({"doc": doc, "set": {"datum.A": [9999]}}));
    assert_eq!(refused["code"], json!("invalid"));
    assert_eq!(refused["question"], json!("datum.A"));
    let after = client.call("questions", json!({"doc": doc, "state": "answered"}));
    assert_eq!(after["questions"].as_array().unwrap().len(), 2);

    // review: open and problems == serve's questions with these answers.
    let reviewed = client.call("review", json!({"doc": doc}));
    let served = serve_ok(
        &mut serve,
        json!({"op": "questions", "analysis": analysis, "answers": answers}),
    );
    assert_eq!(reviewed["open"], served["open"]);
    assert_eq!(reviewed["problems"], served["problems"]);
    assert_eq!(reviewed["writable_with_defaults"], json!(true));
    assert_eq!(
        reviewed["writable"],
        json!(reviewed["missing"].as_array().unwrap().is_empty())
    );
    assert_eq!(reviewed["missing_with_defaults"], json!([]));

    // characteristics == serve's preview intent.
    let previewed = client.call("characteristics", json!({"doc": doc}));
    let served = serve_ok(
        &mut serve,
        json!({"op": "preview", "analysis": analysis, "answers": answers}),
    );
    assert_eq!(previewed["intent"], served["intent"]);
    assert_eq!(previewed["intent"]["part"]["material"], json!("6082-T6"));

    // read_pmi: the analysis' existing PMI and settings.
    let pmi = client.call("read_pmi", json!({"doc": doc}));
    assert_eq!(pmi["existing"], analysis["existing"]);
    assert_eq!(pmi["part_settings"], analysis["part"]);
    assert_eq!(pmi["resumed"], json!(false));

    // write_pmi == serve's write.
    let out = dir.join("plate-pmi.step");
    let written = client.call(
        "write_pmi",
        json!({"doc": doc, "output": out, "accept_defaults": true}),
    );
    let served_out = dir.join("plate-serve.step");
    let served = serve_ok(
        &mut serve,
        json!({
            "op": "write", "step": plate, "analysis": analysis, "answers": answers,
            "output": served_out, "accept_defaults": true,
        }),
    );
    assert_eq!(
        report_without_output(&written["report"]),
        report_without_output(&served["report"])
    );
    assert!(
        std::fs::read_to_string(&out)
            .unwrap()
            .starts_with("ISO-10303-21")
    );
    // Not again without overwrite.
    let refused = client.refusal(
        "write_pmi",
        json!({"doc": doc, "output": out, "accept_defaults": true}),
    );
    assert_eq!(refused["code"], json!("refused"));

    // export_answers == serve's export, and saved where asked.
    let sidecar = dir.join("plate.answers.json");
    let exported = client.call("export_answers", json!({"doc": doc, "output": sidecar}));
    let served = serve_ok(
        &mut serve,
        json!({
            "op": "export_answers", "step": plate, "analysis": analysis, "answers": answers,
        }),
    );
    assert_eq!(exported["answers"], served["answers"]);
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&sidecar).unwrap()).unwrap();
    assert_eq!(saved, served["answers"]);

    // diff of two revisions == the revision report the API gives.
    let (old, new) = (
        revision("plate_split.old.step.gz"),
        revision("plate_split.new.step.gz"),
    );
    let compared = client.call("diff", json!({"old": old, "new": new}));
    let expected = specify_core::revision::diff(
        &specify_core::part::load(&old, None).unwrap(),
        &specify_core::part::load(&new, None).unwrap(),
        &Map::new(),
    )
    .unwrap();
    assert_eq!(compared["kind"], json!("part"));
    assert_eq!(compared["verdict"], json!(expected.verdict));
    assert_eq!(compared["summary"], json!(expected.summary));
    assert_eq!(compared["report"], serde_json::to_value(&expected).unwrap());

    // A resource of no open document: not found, by each era's code.
    let missing = client.request(
        "resources/read",
        json!({"uri": "specify://doc/nothing-0/analysis"}),
    );
    let code = if era == mcp::MODERN { -32602 } else { -32002 };
    assert_eq!(missing["error"]["code"], json!(code), "{missing}");

    client.finish();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_conversation_in_2026_07_28() {
    conversation(mcp::MODERN);
}

#[test]
fn a_conversation_in_2025_11_25() {
    conversation(mcp::LEGACY);
}

// ----- refusals, in memory -----

fn modern(id: i64, method: &str, params: Value) -> Value {
    let mut params = params.as_object().cloned().unwrap_or_default();
    params.insert(
        "_meta".into(),
        json!({META_VERSION: mcp::MODERN, META_CAPABILITIES: {}}),
    );
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

fn call(server: &mut Server, name: &str, arguments: Value) -> Value {
    let reply = server.handle(
        &modern(
            1,
            "tools/call",
            json!({"name": name, "arguments": arguments}),
        ),
        None,
    );
    reply.body.unwrap()["result"].clone()
}

fn error_of(server: &mut Server, name: &str, arguments: Value) -> Value {
    let result = call(server, name, arguments);
    assert_eq!(result["isError"], json!(true), "{result}");
    result["structuredContent"]["error"].clone()
}

#[test]
fn an_unsupported_version_is_refused_with_the_supported_list() {
    let mut server = Server::new(&[scratch("version")]).unwrap();
    let request = json!({"jsonrpc": "2.0", "id": 7, "method": "tools/list", "params": {
        "_meta": {META_VERSION: "1900-01-01", META_CAPABILITIES: {}}}});
    let reply = server.handle(&request, None);
    assert_eq!(reply.status, 400);
    let body = reply.body.unwrap();
    assert_eq!(body["id"], json!(7));
    assert_eq!(body["error"]["code"], json!(-32022));
    assert_eq!(
        body["error"]["data"]["supported"],
        json!([mcp::MODERN, mcp::LEGACY])
    );
    assert_eq!(body["error"]["data"]["requested"], json!("1900-01-01"));
    // No version at all, and no initialize: invalid params.
    let bare = json!({"jsonrpc": "2.0", "id": 8, "method": "tools/list"});
    assert_eq!(
        server.handle(&bare, None).body.unwrap()["error"]["code"],
        json!(-32602)
    );
    // A modern request without client capabilities is malformed too.
    let partial = json!({"jsonrpc": "2.0", "id": 9, "method": "tools/list",
                         "params": {"_meta": {META_VERSION: mcp::MODERN}}});
    assert_eq!(
        server.handle(&partial, None).body.unwrap()["error"]["code"],
        json!(-32602)
    );
    // An unknown method, and an unknown tool.
    let unknown = server.handle(&modern(10, "prompts/list", json!({})), None);
    assert_eq!(unknown.status, 404);
    assert_eq!(unknown.body.unwrap()["error"]["code"], json!(-32601));
    let tool = server.handle(&modern(11, "tools/call", json!({"name": "bogus"})), None);
    assert_eq!(tool.body.unwrap()["error"]["code"], json!(-32602));
}

#[test]
fn paths_outside_allow_are_refused_even_through_a_symbolic_link() {
    let dir = scratch("allow");
    let mut server = Server::new(std::slice::from_ref(&dir)).unwrap();
    let refused = error_of(&mut server, "list_parts", json!({"path": part("plate")}));
    assert_eq!(refused["code"], json!("refused"), "{refused}");
    let refused = error_of(&mut server, "open", json!({"path": part("plate")}));
    assert_eq!(refused["code"], json!("refused"));
    #[cfg(unix)]
    {
        let link = dir.join("plate.step");
        std::os::unix::fs::symlink(part("plate"), &link).unwrap();
        let refused = error_of(&mut server, "list_parts", json!({"path": link}));
        assert_eq!(refused["code"], json!("refused"), "{refused}");
        let traversal = dir.join("..").join("x.step");
        let refused = error_of(&mut server, "diff", json!({"old": traversal, "new": link}));
        assert!(["refused", "invalid"].contains(&refused["code"].as_str().unwrap()));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_and_stale_handles_are_refused_and_overwriting_needs_consent() {
    let dir = scratch("handles");
    let mut server = Server::new(std::slice::from_ref(&dir)).unwrap();
    let unknown = error_of(&mut server, "questions", json!({"doc": "0123-0"}));
    assert_eq!(unknown["code"], json!("unknown_document"));
    assert!(unknown["message"].as_str().unwrap().contains("restart"));

    let copy = dir.join("plate.step");
    std::fs::copy(part("plate"), &copy).unwrap();
    let answers = dir.join("answers.json");
    let opened = call(
        &mut server,
        "open",
        json!({"path": copy, "answers_file": answers}),
    );
    let doc = opened["structuredContent"]["doc"]
        .as_str()
        .unwrap()
        .to_string();
    let set = call(
        &mut server,
        "answer",
        json!({"doc": doc, "set": {"part.material": "6061-T6"}}),
    );
    assert_eq!(set["isError"], json!(false), "{set}");
    let kept: Value = serde_json::from_str(&std::fs::read_to_string(&answers).unwrap()).unwrap();
    assert_eq!(kept, json!({"part.material": "6061-T6"}));

    // The input is never overwritten without consent, nor written as an answers file.
    let refused = error_of(
        &mut server,
        "write_pmi",
        json!({"doc": doc, "output": copy, "accept_defaults": true}),
    );
    assert_eq!(refused["code"], json!("refused"));
    let refused = error_of(
        &mut server,
        "answer",
        json!({"doc": doc, "answers_file": copy}),
    );
    assert_eq!(refused["code"], json!("refused"));

    // A file changed since it was opened is stale.
    let mut text = std::fs::read(&copy).unwrap();
    text.extend_from_slice(b"\n");
    std::fs::write(&copy, text).unwrap();
    let stale = error_of(&mut server, "review", json!({"doc": doc}));
    assert_eq!(stale["code"], json!("stale"), "{stale}");

    // A new server (a restart) has forgotten the handle; the answers file brings them back.
    let mut restarted = Server::new(std::slice::from_ref(&dir)).unwrap();
    let forgotten = error_of(&mut restarted, "questions", json!({"doc": doc}));
    assert_eq!(forgotten["code"], json!("unknown_document"));
    let reopened = call(
        &mut restarted,
        "open",
        json!({"path": copy, "answers_file": answers}),
    );
    assert_eq!(
        reopened["structuredContent"]["answered"],
        json!(1),
        "{reopened}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_assembly_opened_without_a_part_lists_its_parts() {
    let mut server = Server::new(&[common::fixtures()]).unwrap();
    let opened = call(
        &mut server,
        "open",
        json!({"path": part("a_file_with_two_parts_must_say_which")}),
    );
    let out = &opened["structuredContent"];
    assert_eq!(out["needs_part"], json!(true), "{out}");
    assert_eq!(out["doc"], json!(null));
    assert_eq!(out["parts"].as_array().unwrap().len(), 2);
}

// ----- HTTP -----

/// A raw HTTP exchange: the status code and the body.
fn http(port: u16, head: &str, body: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(stream, "{head}Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    let mut response = String::new();
    BufReader::new(stream)
        .read_to_string(&mut response)
        .unwrap();
    let status = response.split(' ').nth(1).unwrap().parse().unwrap();
    let body = response
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or_default()
        .to_string();
    (status, body)
}

fn start_http(allow: &Path) -> (u16, String) {
    let listener = mcp::bind(0).unwrap();
    let port = listener.local_addr().unwrap().port();
    let token = mcp::new_token().unwrap();
    let server = Server::new(&[allow.to_path_buf()]).unwrap();
    let t = token.clone();
    thread::spawn(move || mcp::serve_http(listener, t, server));
    (port, token)
}

#[test]
fn http_needs_the_token_the_host_and_no_origin() {
    let dir = scratch("http");
    let (port, token) = start_http(&dir);
    let discover = modern(1, "server/discover", json!({})).to_string();
    let head = |auth: &str, host: &str, extra: &str, method: &str| {
        format!(
            "POST /mcp HTTP/1.1\r\nHost: {host}\r\n{auth}{extra}Content-Type: application/json\r\n\
             Accept: application/json, text/event-stream\r\nMCP-Protocol-Version: {}\r\nMcp-Method: {method}\r\n",
            mcp::MODERN
        )
    };
    let auth = format!("Authorization: Bearer {token}\r\n");
    let host = format!("127.0.0.1:{port}");

    let (status, body) = http(port, &head(&auth, &host, "", "server/discover"), &discover);
    assert_eq!(status, 200, "{body}");
    let reply: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(reply["result"]["supportedVersions"][0], json!(mcp::MODERN));
    // localhost:<port> is this address too.
    let (status, _) = http(
        port,
        &head(&auth, &format!("localhost:{port}"), "", "server/discover"),
        &discover,
    );
    assert_eq!(status, 200);

    // No token, or the wrong one.
    assert_eq!(
        http(port, &head("", &host, "", "server/discover"), &discover).0,
        401
    );
    assert_eq!(
        http(
            port,
            &head("Authorization: Bearer 00\r\n", &host, "", "server/discover"),
            &discover
        )
        .0,
        401
    );
    // Another Host (DNS rebinding).
    assert_eq!(
        http(
            port,
            &head(
                &auth,
                &format!("evil.example:{port}"),
                "",
                "server/discover"
            ),
            &discover
        )
        .0,
        403
    );
    // A browser page.
    assert_eq!(
        http(
            port,
            &head(
                &auth,
                &host,
                "Origin: https://evil.example\r\n",
                "server/discover"
            ),
            &discover
        )
        .0,
        403
    );
    // Headers that do not match the body.
    let (status, body) = http(port, &head(&auth, &host, "", "tools/list"), &discover);
    assert_eq!(status, 400);
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["error"]["code"],
        json!(-32020)
    );
    // One tool call, its name mirrored.
    let call = modern(
        2,
        "tools/call",
        json!({"name": "list_parts", "arguments": {"path": "nowhere.step"}}),
    )
    .to_string();
    let (status, body) = http(
        port,
        &format!(
            "{}Mcp-Name: list_parts\r\n",
            head(&auth, &host, "", "tools/call")
        ),
        &call,
    );
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["result"]["isError"],
        json!(true)
    );
    // A notification is accepted without a body; GET is not served.
    let note =
        json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {}}).to_string();
    assert_eq!(
        http(
            port,
            &head(&auth, &host, "", "notifications/cancelled"),
            &note
        )
        .0,
        202
    );
    let get = format!("GET /mcp HTTP/1.1\r\nHost: {host}\r\n{auth}");
    assert_eq!(http(port, &get, "").0, 405);
    // A legacy client: initialize, then its version in the header.
    let init = json!({"jsonrpc": "2.0", "id": 3, "method": "initialize", "params": {
        "protocolVersion": mcp::LEGACY, "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}}}).to_string();
    let legacy =
        format!("POST /mcp HTTP/1.1\r\nHost: {host}\r\n{auth}Content-Type: application/json\r\n");
    let (status, body) = http(port, &legacy, &init);
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["result"]["protocolVersion"],
        json!(mcp::LEGACY)
    );
    let list = json!({"jsonrpc": "2.0", "id": 4, "method": "tools/list"}).to_string();
    let (status, body) = http(
        port,
        &format!("{legacy}MCP-Protocol-Version: {}\r\n", mcp::LEGACY),
        &list,
    );
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["result"]["tools"]
            .as_array()
            .unwrap()
            .len(),
        10
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The binary over real pipes: only protocol messages on stdout, and exit 0 when stdin closes.
#[test]
fn the_binary_speaks_only_protocol_on_stdout() {
    use std::process::{Command, Stdio as Piped};
    let dir = scratch("binary");
    let mut child = Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .args(["mcp", "--allow"])
        .arg(&dir)
        .stdin(Piped::piped())
        .stdout(Piped::piped())
        .stderr(Piped::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, "{}", modern(1, "server/discover", json!({}))).unwrap();
    writeln!(stdin, "not json").unwrap();
    writeln!(
        stdin,
        "{}",
        modern(
            2,
            "tools/call",
            json!({"name": "open", "arguments": {"path": "missing.step"}})
        )
    )
    .unwrap();
    drop(stdin);
    let mut lines = Vec::new();
    for line in BufReader::new(child.stdout.take().unwrap()).lines() {
        lines.push(serde_json::from_str::<Value>(&line.unwrap()).unwrap());
    }
    assert!(child.wait().unwrap().success());
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["result"]["resultType"], json!("complete"));
    assert_eq!(lines[1]["error"]["code"], json!(-32700));
    assert_eq!(lines[2]["result"]["isError"], json!(true));
    let _ = std::fs::remove_dir_all(&dir);
}
