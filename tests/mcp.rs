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
use std::time::{Duration, Instant};

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
        Stdio::start_with(allow, era, |_| {})
    }

    /// [`Stdio::start`] with the server configured first (the app `show` serves, say).
    fn start_with(
        allow: &[PathBuf],
        era: &'static str,
        configure: impl FnOnce(&mut Server) + Send + 'static,
    ) -> Self {
        let (to_server, lines) = channel::<String>();
        let (out, from_server) = channel::<Vec<u8>>();
        let allow = allow.to_vec();
        let server = thread::spawn(move || {
            let mut server = Server::new(&allow).unwrap();
            configure(&mut server);
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
            "diff",
            "changes",
            "show",
            "save"
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
    }
    // `..` out of the allowed directory, to a file that exists: refused by --allow.
    let outside = scratch("allow-outside");
    let target = outside.join("x.step");
    std::fs::copy(part("plate"), &target).unwrap();
    let traversal = dir
        .join("..")
        .join(outside.file_name().unwrap())
        .join("x.step");
    assert!(traversal.exists());
    let inside = dir.join("inside.step");
    std::fs::copy(part("plate"), &inside).unwrap();
    let refused = error_of(
        &mut server,
        "diff",
        json!({"old": traversal, "new": inside}),
    );
    assert_eq!(refused["code"], json!("refused"), "{refused}");
    assert!(
        refused["message"].as_str().unwrap().contains("--allow"),
        "{refused}"
    );
    assert_eq!(refused["path"], json!(target));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&outside);
}

/// An output that is a symbolic link out of `--allow` is refused, even with overwrite, and
/// the file it points at is left as it was.
#[cfg(unix)]
#[test]
fn an_output_link_out_of_allow_is_refused() {
    let dir = scratch("output-link");
    let outside = scratch("output-link-outside");
    let victim = outside.join("victim.step");
    std::fs::write(&victim, b"not to be replaced\n").unwrap();
    let copy = dir.join("plate.step");
    std::fs::copy(part("plate"), &copy).unwrap();
    let link = dir.join("out.step");
    std::os::unix::fs::symlink(&victim, &link).unwrap();
    let json_link = dir.join("answers.json");
    std::os::unix::fs::symlink(outside.join("answers.json"), &json_link).unwrap();
    std::fs::write(outside.join("answers.json"), b"{}\n").unwrap();

    let mut server = Server::new(std::slice::from_ref(&dir)).unwrap();
    let opened = call(&mut server, "open", json!({"path": copy}));
    let doc = opened["structuredContent"]["doc"]
        .as_str()
        .unwrap()
        .to_string();
    for (tool, arguments) in [
        (
            "write_pmi",
            json!({"doc": doc, "output": link, "accept_defaults": true, "overwrite": true}),
        ),
        (
            "export_answers",
            json!({"doc": doc, "output": link, "overwrite": true}),
        ),
        (
            "answer",
            json!({"doc": doc, "set": {"part.material": "6061-T6"}, "answers_file": json_link}),
        ),
    ] {
        let refused = error_of(&mut server, tool, arguments);
        assert_eq!(refused["code"], json!("refused"), "{tool}: {refused}");
        assert!(
            refused["message"].as_str().unwrap().contains("--allow"),
            "{tool}: {refused}"
        );
    }
    assert_eq!(std::fs::read(&victim).unwrap(), b"not to be replaced\n");
    assert_eq!(
        std::fs::read(outside.join("answers.json")).unwrap(),
        b"{}\n"
    );
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&outside);
}

/// An existing file that is not an answer map is never replaced as an answers_file without
/// overwrite; an existing answer map is loaded and kept up to date.
#[test]
fn an_answers_file_replaces_only_an_answer_map_without_overwrite() {
    let dir = scratch("answers-file");
    let copy = dir.join("plate.step");
    std::fs::copy(part("plate"), &copy).unwrap();
    let notes = dir.join("notes.md");
    std::fs::write(&notes, b"# my notes\n").unwrap();
    let other = dir.join("package.json");
    std::fs::write(&other, b"{\"name\": \"x\", \"version\": \"1\"}\n").unwrap();
    let mut server = Server::new(std::slice::from_ref(&dir)).unwrap();

    // open refuses it, and opens nothing.
    let refused = error_of(
        &mut server,
        "open",
        json!({"path": copy, "answers_file": notes}),
    );
    assert_eq!(refused["code"], json!("refused"), "{refused}");
    assert!(
        refused["message"].as_str().unwrap().contains("notes.md"),
        "{refused}"
    );
    assert_eq!(refused["path"], json!(notes));
    assert_eq!(std::fs::read(&notes).unwrap(), b"# my notes\n");

    let opened = call(&mut server, "open", json!({"path": copy}));
    let doc = opened["structuredContent"]["doc"]
        .as_str()
        .unwrap()
        .to_string();
    // answer refuses it, changes nothing, and leaves both files byte-identical.
    for file in [&notes, &other] {
        let refused = error_of(
            &mut server,
            "answer",
            json!({"doc": doc, "set": {"part.material": "6061-T6"}, "answers_file": file}),
        );
        assert_eq!(refused["code"], json!("refused"), "{refused}");
        assert!(
            refused["message"]
                .as_str()
                .unwrap()
                .contains(file.file_name().unwrap().to_str().unwrap())
        );
    }
    assert_eq!(std::fs::read(&notes).unwrap(), b"# my notes\n");
    assert_eq!(
        std::fs::read(&other).unwrap(),
        b"{\"name\": \"x\", \"version\": \"1\"}\n"
    );
    let unchanged = call(
        &mut server,
        "questions",
        json!({"doc": doc, "state": "answered"}),
    );
    assert_eq!(
        unchanged["structuredContent"]["questions"],
        json!([]),
        "{unchanged}"
    );

    // An existing answer map is loaded on open and updated by answer.
    let kept = dir.join("plate.answers.json");
    std::fs::write(&kept, b"{\"part.material\": \"6082-T6\"}\n").unwrap();
    let reopened = call(
        &mut server,
        "open",
        json!({"path": copy, "answers_file": kept}),
    );
    assert_eq!(reopened["isError"], json!(false), "{reopened}");
    assert_eq!(reopened["structuredContent"]["answered"], json!(1));
    let set = call(
        &mut server,
        "answer",
        json!({"doc": doc, "set": {"part.material": "6061-T6"}}),
    );
    assert_eq!(set["isError"], json!(false), "{set}");
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&kept).unwrap()).unwrap();
    assert_eq!(saved, json!({"part.material": "6061-T6"}));

    // With overwrite: true the other file is replaced by the answers.
    let set = call(
        &mut server,
        "answer",
        json!({"doc": doc, "answers_file": notes, "overwrite": true}),
    );
    assert_eq!(set["isError"], json!(false), "{set}");
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&notes).unwrap()).unwrap();
    assert_eq!(saved, json!({"part.material": "6061-T6"}));
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
        13
    );
    // A legacy-era JSON-RPC error is a 200 body (2025-11-25 Streamable HTTP); an
    // unsupported header version is still 400.
    let unknown = json!({"jsonrpc": "2.0", "id": 5, "method": "prompts/list"}).to_string();
    let (status, body) = http(
        port,
        &format!("{legacy}MCP-Protocol-Version: {}\r\n", mcp::LEGACY),
        &unknown,
    );
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["error"]["code"],
        json!(-32601)
    );
    let (status, body) = http(
        port,
        &format!("{legacy}MCP-Protocol-Version: 1900-01-01\r\n"),
        &list,
    );
    assert_eq!(status, 400, "{body}");
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["error"]["code"],
        json!(-32022)
    );
    // The same unknown method in 2026-07-28 is 404.
    let (status, body) = http(
        port,
        &head(&auth, &host, "", "prompts/list"),
        &modern(6, "prompts/list", json!({})).to_string(),
    );
    assert_eq!(status, 404, "{body}");
    // A 2026-07-28 header with no version in the body: the header does not match the body
    // (-32020), never "unsupported" for a version the server speaks.
    let (status, body) = http(port, &head(&auth, &host, "", "tools/list"), &list);
    assert_eq!(status, 400, "{body}");
    let error = &serde_json::from_str::<Value>(&body).unwrap()["error"];
    assert_eq!(error["code"], json!(-32020), "{body}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The scheme name is case-insensitive, and a real tool call succeeds over HTTP.
#[test]
fn http_serves_a_tool_call_with_any_case_of_bearer() {
    let dir = scratch("http-call");
    let copy = dir.join("plate.step");
    std::fs::copy(part("plate"), &copy).unwrap();
    let (port, token) = start_http(&dir);
    let host = format!("127.0.0.1:{port}");
    let call = modern(
        1,
        "tools/call",
        json!({"name": "list_parts", "arguments": {"path": copy}}),
    )
    .to_string();
    for scheme in ["Bearer", "bearer", "BEARER"] {
        let head = format!(
            "POST /mcp HTTP/1.1\r\nHost: {host}\r\nAuthorization: {scheme} {token}\r\n\
             Content-Type: application/json\r\nAccept: application/json, text/event-stream\r\n\
             MCP-Protocol-Version: {}\r\nMcp-Method: tools/call\r\nMcp-Name: list_parts\r\n",
            mcp::MODERN
        );
        let (status, body) = http(port, &head, &call);
        assert_eq!(status, 200, "{scheme}: {body}");
        let reply: Value = serde_json::from_str(&body).unwrap();
        let result = &reply["result"];
        assert_eq!(result["isError"], json!(false), "{body}");
        assert_eq!(result["resultType"], json!("complete"));
        assert_eq!(
            result["structuredContent"]["parts"]
                .as_array()
                .unwrap()
                .len(),
            1,
            "{body}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Stalled connections hold at most MAX_CONNECTIONS threads; beyond that a connection is
/// answered 503, and the server serves again once they go.
#[test]
fn http_caps_its_connections() {
    let dir = scratch("http-cap");
    let (port, token) = start_http(&dir);
    let stalled: Vec<TcpStream> = (0..mcp::MAX_CONNECTIONS)
        .map(|_| TcpStream::connect(("127.0.0.1", port)).unwrap())
        .collect();
    // One more: answered at once, without sending anything.
    let mut extra = TcpStream::connect(("127.0.0.1", port)).unwrap();
    extra
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    let mut response = String::new();
    extra.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 503"), "{response}");
    drop(stalled);
    let host = format!("127.0.0.1:{port}");
    let head = format!(
        "POST /mcp HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {token}\r\n\
         MCP-Protocol-Version: {}\r\nMcp-Method: server/discover\r\n",
        mcp::MODERN
    );
    let discover = modern(1, "server/discover", json!({})).to_string();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        // A 503 is sent without reading the request, so the connection may also be reset.
        let attempt = || -> std::io::Result<String> {
            let mut stream = TcpStream::connect(("127.0.0.1", port))?;
            write!(
                stream,
                "{head}Content-Length: {}\r\n\r\n{discover}",
                discover.len()
            )?;
            let mut response = String::new();
            stream.read_to_string(&mut response)?;
            Ok(response)
        };
        match attempt() {
            Ok(r) if r.starts_with("HTTP/1.1 200") => break,
            Ok(r) => assert!(r.starts_with("HTTP/1.1 503"), "{r}"),
            Err(_) => {}
        }
        assert!(std::time::Instant::now() < deadline, "still refused");
        thread::sleep(std::time::Duration::from_millis(20));
    }
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

// ----- the session, shared with the page -----

/// A request to a page server `show` started, from the app's own page.
fn page(url: &str, method: &str, rel: &str, body: Option<&Value>) -> (u16, Value) {
    let rest = url.strip_prefix("http://127.0.0.1:").unwrap();
    let (port, prefix) = rest.split_once('/').unwrap();
    let port: u16 = port.parse().unwrap();
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let body = body.map(Value::to_string).unwrap_or_default();
    write!(
        stream,
        "{method} /{prefix}{rel} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\
         Origin: http://127.0.0.1:{port}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut response = String::new();
    BufReader::new(stream)
        .read_to_string(&mut response)
        .unwrap();
    let status = response.split(' ').nth(1).unwrap().parse().unwrap();
    let body = response.split("\r\n\r\n").nth(1).unwrap_or_default();
    (status, serde_json::from_str(body).unwrap_or(Value::Null))
}

/// An open `events` stream of a page server.
struct Events(BufReader<TcpStream>);

impl Events {
    fn open(url: &str) -> Events {
        let rest = url.strip_prefix("http://127.0.0.1:").unwrap();
        let (port, prefix) = rest.split_once('/').unwrap();
        let mut stream = TcpStream::connect(("127.0.0.1", port.parse::<u16>().unwrap())).unwrap();
        write!(
            stream,
            "GET /{prefix}events HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n"
        )
        .unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        let mut events = Events(BufReader::new(stream));
        let head = events.block();
        assert!(head[0].starts_with("HTTP/1.1 200"), "{head:?}");
        events
    }

    /// The lines up to the next blank one.
    fn block(&mut self) -> Vec<String> {
        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            assert!(self.0.read_line(&mut line).unwrap() > 0, "the stream ended");
            let line = line.trim_end_matches(['\r', '\n']).to_string();
            if line.is_empty() {
                return lines;
            }
            lines.push(line);
        }
    }

    /// The next event (comments skipped): its name and data.
    fn next(&mut self) -> (String, Value) {
        loop {
            let block = self.block();
            let name = block.iter().find_map(|l| l.strip_prefix("event: "));
            let data = block.iter().find_map(|l| l.strip_prefix("data: "));
            if let (Some(name), Some(data)) = (name, data) {
                return (name.to_string(), serde_json::from_str(data).unwrap());
            }
        }
    }
}

/// A stub app for `show` to serve, in `dir`.
fn stub_app(dir: &Path) -> specify_core::open::Assets {
    let app = dir.join("app");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(
        app.join("index.html"),
        b"<!doctype html><title>stub</title>",
    )
    .unwrap();
    specify_core::open::assets(Some(&app), None).unwrap()
}

/// The agent and the person's page edit one session: the page's change is what the agent
/// reads next, the agent's reaches the page as an event, and both save the same way.
#[test]
fn the_agent_and_the_page_share_one_session() {
    let dir = scratch("session");
    let copy = dir.join("plate.step");
    std::fs::copy(part("plate"), &copy).unwrap();
    let assets = stub_app(&dir);
    let mut client = Stdio::start_with(std::slice::from_ref(&dir), mcp::MODERN, move |server| {
        server.set_app(assets);
        server.set_page_timing(Duration::from_millis(500), Duration::from_secs(600));
    });

    let opened = client.call("open", json!({"path": copy}));
    let doc = opened["doc"].as_str().unwrap().to_string();
    assert_eq!(opened["rev"], json!(0));
    let shown = client.call("show", json!({"doc": doc, "browser": false}));
    assert_eq!(shown["mode"], json!("specify"));
    let url = shown["url"].as_str().unwrap().to_string();
    assert!(url.starts_with("http://127.0.0.1:"), "{url}");
    // Shown again while it runs: the same page.
    let again = client.call("show", json!({"doc": doc, "browser": false}));
    assert_eq!(again["url"], json!(url));
    let (status, config) = page(&url, "GET", "config", None);
    assert_eq!(status, 200);
    assert_eq!(config["path"], json!(copy));

    // The person answers in the page: the agent's next calls see it.
    let (status, put) = page(
        &url,
        "PUT",
        "answers",
        Some(&json!({"rev": 0, "part": 0, "answers": {"part.material": "6061-T6"}})),
    );
    assert_eq!(status, 200, "{put}");
    assert_eq!(put, json!({"rev": 1}));
    let changes = client.call("changes", json!({"doc": doc, "since": 0}));
    assert_eq!(
        changes,
        json!({"doc": doc, "rev": 1, "changes": [{"rev": 1, "part": 0, "source": "page",
               "set": {"part.material": "6061-T6"}, "cleared": []}]})
    );
    let answered = client.call("questions", json!({"doc": doc, "state": "answered"}));
    assert_eq!(answered["rev"], json!(1));
    let material = answered["questions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["id"] == "part.material")
        .expect("answered in the page");
    assert_eq!(material["answer"], json!("6061-T6"));

    // The agent answers: the page hears it.
    let mut events = Events::open(&url);
    let (_, start) = events.next();
    assert_eq!(start["source"], json!("start"));
    assert_eq!(start["rev"], json!(1));
    let set = client.call(
        "answer",
        json!({"doc": doc, "set": {"part.material": "6082-T6"}}),
    );
    assert_eq!(set["rev"], json!(2));
    let (name, change) = events.next();
    assert_eq!(name, "answers");
    assert_eq!(
        change,
        json!({"rev": 2, "part": 0, "answers": {"part.material": "6082-T6"}, "source": "mcp"})
    );

    // Save says whether to take the defaults, always; an incomplete save is written.
    let refused = client.refusal("save", json!({"doc": doc}));
    assert_eq!(refused["code"], json!("invalid"), "{refused}");
    assert!(!dir.join("plate.specified.step").exists());
    let saved = client.call("save", json!({"doc": doc, "accept_defaults": false}));
    assert_eq!(saved["complete"], json!(false), "{saved}");
    assert!(!saved["missing"].as_array().unwrap().is_empty());
    assert_eq!(saved["rev"], json!(2));
    assert_eq!(saved["path"], json!(dir.join("plate.specified.step")));
    assert!(
        std::fs::read(dir.join("plate.specified.step"))
            .unwrap()
            .starts_with(b"ISO-10303-21;")
    );
    let (name, told) = events.next();
    assert_eq!(name, "saved");
    assert_eq!(told["source"], json!("mcp"));
    assert_eq!(told["path"], saved["path"]);
    let changes = client.call("changes", json!({"doc": doc, "since": 1}));
    assert_eq!(changes["changes"][0]["source"], json!("mcp"));
    assert_eq!(changes["changes"].as_array().unwrap().len(), 1);

    // The page closed: its server stops, and show starts a new one at a new address.
    drop(events);
    let (status, _) = page(&url, "POST", "closed", None);
    assert_eq!(status, 204);
    let deadline = Instant::now() + Duration::from_secs(10);
    let renewed = loop {
        assert!(Instant::now() < deadline, "the page server did not stop");
        thread::sleep(Duration::from_millis(100));
        let shown = client.call("show", json!({"doc": doc, "browser": false}));
        if shown["url"] != json!(url) {
            break shown["url"].as_str().unwrap().to_string();
        }
    };
    let (status, answers) = page(&renewed, "GET", "answers", None);
    assert_eq!(status, 200);
    assert_eq!(
        answers,
        json!({"rev": 2, "parts": {"0": {"part.material": "6082-T6"}}})
    );
    client.finish();
    let _ = std::fs::remove_dir_all(&dir);
}

/// `open` with `old`: the revisions compared as `diff` compares them, and the session starting
/// from the answers carried to the new one.
#[test]
fn a_diff_session_starts_from_the_carried_answers() {
    let dir = scratch("diff-session");
    let (old, new) = (dir.join("plate.old.step.gz"), dir.join("plate.new.step.gz"));
    std::fs::copy(revision("hole_moved.old.step.gz"), &old).unwrap();
    std::fs::copy(revision("hole_moved.new.step.gz"), &new).unwrap();
    let old_answers = dir.join("old.answers.json");
    std::fs::write(&old_answers, r#"{"part.material": "6061-T6"}"#).unwrap();
    let mut server = Server::new(std::slice::from_ref(&dir)).unwrap();

    let opened = call(
        &mut server,
        "open",
        json!({"path": new, "old": old, "old_answers": old_answers}),
    );
    let out = &opened["structuredContent"];
    assert_eq!(opened["isError"], json!(false), "{opened}");
    let given: Map<String, Value> =
        serde_json::from_str(r#"{"part.material": "6061-T6"}"#).unwrap();
    let expected = specify_core::revision::diff(
        &specify_core::part::load(&old, None).unwrap(),
        &specify_core::part::load(&new, None).unwrap(),
        &given,
    )
    .unwrap();
    assert_eq!(out["verdict"], json!(expected.verdict));
    assert_eq!(out["summary"], json!(expected.summary));
    assert_eq!(out["report"], serde_json::to_value(&expected).unwrap());
    assert_eq!(out["rev"], json!(0));
    assert_eq!(out["answered"], json!(expected.answers.len()));
    let doc = out["doc"].as_str().unwrap().to_string();
    let answered = call(
        &mut server,
        "questions",
        json!({"doc": doc, "state": "answered"}),
    );
    let material = answered["structuredContent"]["questions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["id"] == "part.material")
        .cloned()
        .expect("carried");
    assert_eq!(material["answer"], json!("6061-T6"));
    // old_answers belong to old.
    let refused = error_of(
        &mut server,
        "open",
        json!({"path": new, "old_answers": old_answers}),
    );
    assert_eq!(refused["code"], json!("invalid"));
    let _ = std::fs::remove_dir_all(&dir);
}
