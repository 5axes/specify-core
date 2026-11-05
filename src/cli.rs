//! `specify-core`: the command line (port of `cli.py`). [`main`] runs it on its arguments: the
//! binary (`main.rs`) and the Python package's `specify-core` console script both call it.
//!
//! ```text
//! specify-core parts STEP                        the file's parts, as JSON
//! specify-core analyse STEP -o ANALYSIS [--part K]
//! specify-core mesh STEP -o MESH [--part K]
//! specify-core questions ANALYSIS [--answers ANSWERS] [--open]
//!                        [--resolve-defaults --step STEP]
//! specify-core write STEP ANALYSIS ANSWERS [ANALYSIS ANSWERS ...] -o OUT
//!                    [--accept-defaults] [--intent INTENT]
//! specify-core diff OLD NEW -o DIR [--answers ANSWERS] [--part K] [--title TITLE]
//!                    [--viewer URL]
//!                    what changed between two revisions of a part, its answers carried
//!                    to the new one (revision.rs), or of an assembly, part by part
//!                    (revision/assembly.rs)
//! specify-core serve                             JSON-line requests on stdin (serve.rs)
//! specify-core open STEP [--editor] [--site ORIGIN] [--no-browser]
//!                    the file opened in draftwright.io/specify, served once on 127.0.0.1
//!                    (open.rs)
//! ```
//!
//! Arguments, outputs, exit codes and messages are specify-core's, so a host can run either
//! engine: `analyse` writes the analysis (Python's `json.dumps(indent=1)` text, non-ASCII
//! escaped, so the files compare byte for byte) and prints `{faces, features, existing_pmi}`;
//! `mesh` writes compact JSON and prints `{faces, triangles}`, warning on stderr of faces that
//! did not triangulate; `parts`, `questions` and `write` print indented JSON with non-ASCII
//! kept. Exit 2 with `incomplete: ...`, `invalid answer: ...` or `cannot write: ...` on stderr
//! (in an assembly prefixed `part K: `), or with argparse's usage and error lines for bad
//! arguments (the same texts); exit 1 with `write failed, OUT left as it was: ...` when a write
//! does not verify. Arguments are parsed by hand, as argparse parses them (including its rule
//! that positionals are matched a run at a time between options, so `write S A B -o OUT C D`
//! leaves `C D` unrecognised).
//!
//! `diff` and `open` are this port's own, with no Python counterpart: they are taken as commands
//! but left out of the top-level help and of argparse's list of choices, which stay Python's
//! texts (`tests/cli.rs` compares them with Python's). `specify-core diff -h` and
//! `specify-core open -h` give their help.
//!
//! Where Python fails with an uncaught exception (a file that cannot be read or loaded, an
//! output that cannot be written) it exits 1 with a traceback; this exits 1 with `error:` and
//! the message on stderr. Python's second name for the program, `pmi-assist`, is an alias of the
//! Python package's entry point and has no counterpart for a Rust binary.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::api::{self, Error};
use crate::part;
use crate::pyjson;
use crate::revision::assembly;
use crate::rules::{self, Intent};
use crate::serve;
use crate::writer::WriteError;
use serde_json::{Map, Value, json};

const PROG: &str = "specify-core";

const COMMANDS: [&str; 6] = ["parts", "analyse", "serve", "mesh", "questions", "write"];

const TOP_HELP: &str = "usage: specify-core [-h] {parts,analyse,serve,mesh,questions,write} ...

positional arguments:
  {parts,analyse,serve,mesh,questions,write}
    parts               list an assembly's parts
    analyse             recognise features and read existing PMI
    serve               answer JSON-line requests on stdin (for a host
                        process)
    mesh                triangles ranged by face index, for a picker
    questions           list the questions, given answers so far
    write               write intent as AP242 PMI

options:
  -h, --help            show this help message and exit
";

/// One subcommand's arguments, as its argparse subparser declares them.
struct Command {
    name: &'static str,
    /// Positionals in order: (name in messages, takes one or more).
    positionals: &'static [(&'static str, bool)],
    /// Options: (short, long, takes a value, required).
    options: &'static [(Option<&'static str>, &'static str, bool, bool)],
    help: &'static str,
}

const PARTS: Command = Command {
    name: "parts",
    positionals: &[("step", false)],
    options: &[],
    help: "usage: specify-core parts [-h] step

positional arguments:
  step

options:
  -h, --help  show this help message and exit
",
};

const BOM: Command = Command {
    name: "bom",
    positionals: &[("step", false)],
    options: &[],
    help: "usage: specify-core bom [-h] step\n\nPrint the assembly BOM as JSON.\n",
};

const ANALYSE: Command = Command {
    name: "analyse",
    positionals: &[("step", false)],
    options: &[
        (Some("-o"), "--output", true, true),
        (None, "--part", true, false),
    ],
    help: "usage: specify-core analyse [-h] -o OUTPUT [--part PART] step

positional arguments:
  step

options:
  -h, --help           show this help message and exit
  -o, --output OUTPUT  analysis JSON
  --part PART          which part of an assembly (see parts)
",
};

const MESH: Command = Command {
    name: "mesh",
    positionals: &[("step", false)],
    options: &[
        (Some("-o"), "--output", true, true),
        (None, "--part", true, false),
    ],
    help: "usage: specify-core mesh [-h] -o OUTPUT [--part PART] step

positional arguments:
  step

options:
  -h, --help           show this help message and exit
  -o, --output OUTPUT  mesh JSON
  --part PART          which part of an assembly (see parts)
",
};

const QUESTIONS: Command = Command {
    name: "questions",
    positionals: &[("analysis", false)],
    options: &[
        (None, "--answers", true, false),
        (None, "--open", false, false),
        (None, "--resolve-defaults", false, false),
        (None, "--step", true, false),
    ],
    help: "usage: specify-core questions [-h] [--answers ANSWERS] [--open]
                              [--resolve-defaults --step STEP] analysis

positional arguments:
  analysis

options:
  -h, --help          show this help message and exit
  --answers ANSWERS   answers JSON: {question id: value}
  --open              only unanswered questions
  --resolve-defaults  export complete answers with defaults and face evidence as JSON
  --step STEP         matching STEP part for export (required with --resolve-defaults)
",
};

const WRITE: Command = Command {
    name: "write",
    positionals: &[("step", false), ("ANALYSIS ANSWERS", true)],
    options: &[
        (Some("-o"), "--output", true, true),
        (None, "--accept-defaults", false, false),
        (None, "--bom", false, false),
        (None, "--intent", true, false),
    ],
    help: "usage: specify-core write [-h] -o OUTPUT [--accept-defaults] [--bom] [--intent INTENT]
                          step ANALYSIS ANSWERS [ANALYSIS ANSWERS ...]

positional arguments:
  step
  ANALYSIS ANSWERS     analysis and answers JSON; in an assembly, a pair for
                       each part to write

options:
  -h, --help           show this help message and exit
  -o, --output OUTPUT
  --accept-defaults    take PMI defaults for the rest
  --bom                explicitly accept optional BOM suggestions
  --intent INTENT      also write the intent JSON here
",
};

const DIFF: Command = Command {
    name: "diff",
    positionals: &[("old", false), ("new", false)],
    options: &[
        (Some("-o"), "--output", true, true),
        (None, "--answers", true, false),
        (None, "--part", true, false),
        (None, "--title", true, false),
        (None, "--viewer", true, false),
        (None, "--pmi-scenes", false, false),
    ],
    help: "usage: specify-core diff [-h] -o OUTPUT [--answers ANSWERS] [--part PART]
                         [--title TITLE] [--viewer VIEWER] [--pmi-scenes] old new

positional arguments:
  old                  the STEP file as it was (a part or an assembly)
  new                  the STEP file as it is now

options:
  -h, --help           show this help message and exit
  -o, --output OUTPUT  a directory: answers.json, report.json, report.md,
                       model.png; an assembly's answers.json is by part
  --answers ANSWERS    answers JSON for the old part: {question id: value};
                       for an assembly, {part name: {question id: value}}
  --part PART          compare one part of an assembly (see parts)
  --pmi-scenes         include the compared PMI scenes in report.json
  --title TITLE        the report's title (the new file's name)
  --viewer VIEWER      the /diff page's link for these revisions: an
                       assembly's changed parts link to it with &part=NAME
",
};

const OPEN: Command = Command {
    name: "open",
    positionals: &[("step", false)],
    options: &[
        (None, "--editor", false, false),
        (None, "--site", true, false),
        (None, "--no-browser", false, false),
    ],
    help: "usage: specify-core open [-h] [--editor] [--site SITE] [--no-browser] step

Open a STEP file in draftwright.io/specify in your browser, without uploading it: the
file is served once on 127.0.0.1 and the page fetches it from there. Stops once it is
fetched, or after 10 minutes. The first time, Chrome asks whether draftwright.io
may \"access other apps and services on this device\": Allow lets the page fetch the file.
If it did not open while the prompt was up, stop this (Ctrl-C) and run it again; if you
chose Block, allow it again in draftwright.io's site settings.

positional arguments:
  step          a .step or .stp file, or either gzipped

options:
  -h, --help    show this help message and exit
  --editor      open the full editor (/editor) instead of /specify
  --site SITE   another deployment's origin, such as http://localhost:5173, opened
                and answered as well as draftwright.io
  --no-browser  print the address to open instead of opening a browser
",
};

const SERVE: Command = Command {
    name: "serve",
    positionals: &[],
    options: &[],
    help: "usage: specify-core serve [-h]

options:
  -h, --help  show this help message and exit
",
};

/// How a run ends: an exit code, and what goes to stderr.
struct Exit {
    code: u8,
    message: String,
}

impl Exit {
    fn new(code: u8, message: impl Into<String>) -> Self {
        Exit {
            code,
            message: message.into(),
        }
    }

    /// argparse's error: the parser's usage line(s), then `prog: error: message`, exit 2.
    fn usage(usage: &str, prog: &str, message: &str) -> Self {
        Exit::new(2, format!("{usage}{prog}: error: {message}"))
    }

    /// An uncaught failure (Python's traceback, exit 1).
    fn error(message: impl std::fmt::Display) -> Self {
        Exit::new(1, format!("error: {message}"))
    }
}

/// The usage line(s) of a help text: up to its first blank line.
fn usage_of(help: &str) -> &str {
    help.find("\n\n").map_or(help, |i| &help[..=i])
}

/// The parsed arguments of a subcommand: positionals in order, and each option given (the
/// last value given wins, as argparse's `store` does).
#[derive(Default)]
struct Args {
    positionals: Vec<String>,
    values: Vec<(&'static str, Option<String>)>,
}

impl Args {
    fn value(&self, long: &str) -> Option<&str> {
        self.values
            .iter()
            .rev()
            .find(|(l, _)| *l == long)
            .and_then(|(_, v)| v.as_deref())
    }

    fn flag(&self, long: &str) -> bool {
        self.values.iter().any(|(l, _)| *l == long)
    }
}

/// What parsing a command line came to.
enum Parsed {
    Run(&'static Command, Args),
    Help(&'static str),
}

/// Python's `repr` of a string, as argparse quotes a value in its messages (`'1'`, or `"it's"`
/// when the text has a single quote and no double one).
fn py_repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::from(quote);
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_control() => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Whether argparse would take `arg` for an option rather than a value (its negative-number
/// rule: no option here looks like a number, so `-1` is a value).
fn looks_like_option(arg: &str) -> bool {
    arg.starts_with('-') && arg != "-" && arg[1..].parse::<f64>().is_err()
}

fn parse(argv: &[String]) -> Result<Parsed, Exit> {
    let top = usage_of(TOP_HELP);
    let Some(first) = argv.first() else {
        return Err(Exit::usage(
            top,
            PROG,
            "the following arguments are required: command",
        ));
    };
    if first == "-h" || first == "--help" {
        return Ok(Parsed::Help(TOP_HELP));
    }
    if looks_like_option(first) {
        return Err(Exit::usage(
            top,
            PROG,
            "the following arguments are required: command",
        ));
    }
    let command: &'static Command = match first.as_str() {
        "parts" => &PARTS,
        "bom" => &BOM,
        "analyse" => &ANALYSE,
        "mesh" => &MESH,
        "questions" => &QUESTIONS,
        "write" => &WRITE,
        "diff" => &DIFF,
        "serve" => &SERVE,
        "open" => &OPEN,
        other => {
            let choices: Vec<String> = COMMANDS.iter().map(|c| format!("'{c}'")).collect();
            return Err(Exit::usage(
                top,
                PROG,
                &format!(
                    "argument command: invalid choice: '{other}' (choose from {})",
                    choices.join(", ")
                ),
            ));
        }
    };
    let usage = usage_of(command.help);
    let prog = format!("{PROG} {}", command.name);
    let fail = |message: String| Exit::usage(usage, &prog, &message);
    let mut args = Args::default();
    // Arguments argparse would not take, with their place in `argv` so they are reported in
    // order; and the positionals, each with the run (between options) it was given in.
    let mut unrecognised: Vec<(usize, String)> = Vec::new();
    let mut given: Vec<(usize, usize, String)> = Vec::new();
    let mut run = 0;
    let mut rest = argv[1..].iter().enumerate();
    let mut only_positionals = false;
    while let Some((at, arg)) = rest.next() {
        if only_positionals || !looks_like_option(arg) {
            given.push((run, at, arg.clone()));
            continue;
        }
        if arg == "--" {
            only_positionals = true;
            continue;
        }
        run += 1;
        // `--name=value`, `--name value`, `-o value`, `-o=value`, `-ovalue`; a long name may be
        // shortened to any prefix only it has, as argparse allows. A short option is its first
        // two characters (not bytes: `-é` is an unknown option, not a panic).
        let (name, attached) = if arg.starts_with("--") {
            match arg.split_once('=') {
                Some((n, v)) => (n.to_string(), Some(v.to_string())),
                None => (arg.clone(), None),
            }
        } else {
            match arg.char_indices().nth(2) {
                Some((cut, _)) => {
                    let (n, v) = arg.split_at(cut);
                    // argparse splits `-o=value` at the `=` when `-o` is an option.
                    let v = match v.strip_prefix('=') {
                        Some(after) if command.options.iter().any(|o| o.0 == Some(n)) => after,
                        _ => v,
                    };
                    (n.to_string(), Some(v.to_string()))
                }
                None => (arg.clone(), None),
            }
        };
        // `-h`, or `--help` (or a prefix of it no other option has): the help, at once.
        let help = name == "-h"
            || (name.starts_with("--")
                && "--help".starts_with(&name)
                && !command.options.iter().any(|o| o.1.starts_with(&name)));
        if help {
            return Ok(Parsed::Help(command.help));
        }
        let matches: Vec<&(Option<&str>, &str, bool, bool)> = if name.starts_with("--") {
            match command.options.iter().find(|o| o.1 == name) {
                Some(exact) => vec![exact],
                None => command
                    .options
                    .iter()
                    .filter(|o| o.1.starts_with(&name))
                    .collect(),
            }
        } else {
            command
                .options
                .iter()
                .filter(|o| o.0 == Some(name.as_str()))
                .collect()
        };
        let option = match matches.as_slice() {
            [one] => **one,
            [] => {
                unrecognised.push((at, arg.clone()));
                continue;
            }
            many => {
                let names: Vec<&str> = many.iter().map(|o| o.1).collect();
                return Err(fail(format!(
                    "ambiguous option: {name} could match {}",
                    names.join(", ")
                )));
            }
        };
        let (short, long, takes_value, _) = option;
        let shown = match short {
            Some(s) => format!("{s}/{long}"),
            None => long.to_string(),
        };
        if !takes_value {
            if attached.is_some() {
                return Err(fail(format!(
                    "argument {shown}: ignored explicit argument {}",
                    py_repr(&attached.unwrap_or_default())
                )));
            }
            args.values.push((long, None));
            continue;
        }
        let value = match attached {
            Some(v) => v,
            None => match rest.clone().next().map(|(_, next)| next) {
                Some(next) if !looks_like_option(next) => {
                    rest.next();
                    next.clone()
                }
                _ => return Err(fail(format!("argument {shown}: expected one argument"))),
            },
        };
        if long == "--part" && value.trim().parse::<i64>().is_err() {
            return Err(fail(format!(
                "argument --part: invalid int value: {}",
                py_repr(&value)
            )));
        }
        args.values.push((long, Some(value)));
    }
    // Positionals, as argparse matches them: each run between options fills as many of the
    // positionals still unfilled as it can (one each, one that takes several taking the rest of
    // the run, so it never continues past an option); what a run has left over is unrecognised.
    let mut wanted = command.positionals.iter().peekable();
    let mut missing: Vec<String> = Vec::new();
    let mut given = given.into_iter().peekable();
    while let Some(&(this_run, _, _)) = given.peek() {
        while let Some((_, at, value)) = given.next_if(|g| g.0 == this_run) {
            match wanted.peek() {
                None => unrecognised.push((at, value)),
                Some(&&(_, several)) => {
                    args.positionals.push(value);
                    // One that takes several is filled once its run ends.
                    if !several || given.peek().is_none_or(|g| g.0 != this_run) {
                        wanted.next();
                    }
                }
            }
        }
    }
    missing.extend(wanted.map(|&(name, _)| name.to_string()));
    for &(short, long, _, required) in command.options {
        if required && !args.flag(long) {
            missing.push(match short {
                Some(s) => format!("{s}/{long}"),
                None => long.to_string(),
            });
        }
    }
    if !missing.is_empty() {
        return Err(fail(format!(
            "the following arguments are required: {}",
            missing.join(", ")
        )));
    }
    if !unrecognised.is_empty() {
        unrecognised.sort();
        let unrecognised: Vec<String> = unrecognised.into_iter().map(|(_, a)| a).collect();
        return Err(Exit::usage(
            top,
            PROG,
            &format!("unrecognized arguments: {}", unrecognised.join(" ")),
        ));
    }
    Ok(Parsed::Run(command, args))
}

/// Runs the command line on `argv` (the arguments after the program's name) and returns the
/// exit code; replies go to stdout, messages to stderr.
pub fn main(argv: &[String]) -> u8 {
    let mut stdout = io::stdout().lock();
    let result = match parse(argv) {
        Ok(Parsed::Help(text)) => {
            let _ = stdout.write_all(text.as_bytes());
            return 0;
        }
        Ok(Parsed::Run(command, args)) => run(command, &args, &mut stdout),
        Err(exit) => Err(exit),
    };
    let _ = stdout.flush();
    match result {
        Ok(()) => 0,
        Err(exit) => {
            eprintln!("{}", exit.message);
            exit.code
        }
    }
}

fn run(command: &Command, args: &Args, out: &mut impl Write) -> Result<(), Exit> {
    let positional = |i: usize| PathBuf::from(&args.positionals[i]);
    let output = || PathBuf::from(args.value("--output").expect("required"));
    match command.name {
        "serve" => {
            let stdin = io::stdin().lock();
            serve::serve(stdin, out).map_err(Exit::error)
        }
        "bom" => {
            let rows = crate::bom::read_path(&positional(0)).map_err(Exit::error)?;
            print(
                out,
                &serde_json::to_string_pretty(&rows).map_err(Exit::error)?,
            )
        }
        "parts" => {
            let parts = api::parts(&positional(0)).map_err(Exit::error)?;
            print(out, &indent1_keeping_non_ascii(&Value::Array(parts)))
        }
        "analyse" => {
            let step = positional(0);
            let part = part_arg(&step, args)?;
            // The file is read once, for the part and the other parts its mates need.
            let file = part::File::read(&step).map_err(Exit::error)?;
            let loaded = file.load(part).map_err(Exit::error)?;
            let (analysis, warnings) =
                api::analyse_reporting_in(&file, &loaded).map_err(Exit::error)?;
            save(&output(), &pyjson::dumps_indent1(&analysis))?;
            print(out, &pyjson::dumps(&summary(&analysis)))?;
            for line in warnings {
                eprintln!("warning: {line}");
            }
            Ok(())
        }
        "mesh" => {
            let step = positional(0);
            let part = part_arg(&step, args)?;
            let mesh = api::mesh_file(&step, part).map_err(Exit::error)?;
            save(&output(), &pyjson::dumps_compact(&mesh))?;
            let count = |key: &str| mesh[key].as_array().map_or(0, Vec::len);
            let shown = json!({"faces": count("faces"), "triangles": count("indices") / 3});
            print(out, &pyjson::dumps(&shown))?;
            let missing: Vec<String> = mesh["missing"]
                .as_array()
                .into_iter()
                .flatten()
                .map(Value::to_string)
                .collect();
            if !missing.is_empty() {
                eprintln!(
                    "warning: faces [{}] did not triangulate",
                    missing.join(", ")
                );
            }
            Ok(())
        }
        "questions" => {
            let analysis = read_json(&positional(0))?;
            let answers = match args.value("--answers") {
                Some(path) => answers_of(read_json(Path::new(path))?)?,
                None => Map::new(),
            };
            if args.flag("--resolve-defaults") {
                if args.flag("--open") {
                    return Err(Exit::new(
                        2,
                        "--open cannot be combined with --resolve-defaults",
                    ));
                }
                let step = args.value("--step").ok_or_else(|| {
                    Exit::new(
                        2,
                        "--resolve-defaults requires --step STEP for face evidence",
                    )
                })?;
                let index = analysis["binding"]["part"].as_u64().unwrap_or(0) as usize;
                let loaded = part::load(Path::new(step), Some(index)).map_err(Exit::error)?;
                let exported = api::export_answers(&loaded, &analysis, &answers)
                    .map_err(|e| Exit::new(2, e.to_string()))?;
                return print(
                    out,
                    &serde_json::to_string_pretty(&exported).map_err(Exit::error)?,
                );
            }
            if args.value("--step").is_some() {
                return Err(Exit::new(2, "--step requires --resolve-defaults"));
            }
            let qs = api::questions(&analysis, &answers).map_err(refused)?;
            let shown: Vec<Value> = if args.flag("--open") {
                rules::open_questions(&qs, &answers)
                    .iter()
                    .map(|q| q.to_dict())
                    .collect()
            } else {
                qs.iter().map(|q| q.to_dict()).collect()
            };
            print(out, &indent1_keeping_non_ascii(&Value::Array(shown)))
        }
        "write" => write(&positional(0), &args.positionals[1..], args, out),
        "diff" => diff(&positional(0), &positional(1), args, out),
        "open" => open(&positional(0), args, out),
        other => unreachable!("{other} is not a command"),
    }
}

/// `write`: each pair's intent applied (refusals naming the part in an assembly), the intents
/// saved when asked, then written: one part on its own, several together.
fn write(step: &Path, pairs: &[String], args: &Args, out: &mut impl Write) -> Result<(), Exit> {
    if !pairs.len().is_multiple_of(2) {
        return Err(Exit::usage(
            usage_of(TOP_HELP),
            PROG,
            "write takes an analysis and its answers for each part",
        ));
    }
    let several = pairs.len() > 2;
    let mut written: Vec<(Intent, Option<Map<String, Value>>)> = Vec::new();
    for pair in pairs.chunks(2) {
        let analysis = read_json(Path::new(&pair[0]))?;
        let answers = answers_of(read_json(Path::new(&pair[1]))?)?;
        if answers.contains_key(crate::answer_file::META) {
            let index = analysis["binding"]["part"].as_u64().unwrap_or(0) as usize;
            let loaded = part::load(step, Some(index)).map_err(Exit::error)?;
            crate::answer_file::check(&loaded, &answers)
                .map_err(|why| Exit::new(2, format!("answers {}: {why}", pair[1])))?;
        }
        let mut intent = match api::apply(&analysis, &answers, args.flag("--accept-defaults")) {
            Ok(intent) => intent,
            Err(e) if !several => return Err(refused(e)),
            Err(e) => {
                // Which part of the assembly the answers are for.
                let part = analysis
                    .get("binding")
                    .and_then(|b| b.get("part"))
                    .cloned()
                    .unwrap_or(json!(0));
                return Err(refused(match e {
                    Error::Rules(rules::Error::Incomplete(mut e)) => {
                        e.part = Some(part.as_u64().unwrap_or(0) as usize);
                        Error::from(e)
                    }
                    Error::Rules(rules::Error::Invalid(m)) => {
                        let shown = match &part {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        Error::Rules(rules::Error::Invalid(format!("part {shown}: {m}")))
                    }
                    other => other,
                }));
            }
        };
        if args.flag("--bom") {
            crate::bom::accept_suggestions(&analysis, &answers, &mut intent)
                .map_err(Exit::error)?;
        }
        written.push((intent, Some(answers)));
    }
    if let Some(path) = args.value("--intent") {
        let intents: Vec<Value> = written.iter().map(|(i, _)| i.to_dict()).collect();
        let shown = match intents.as_slice() {
            [one] => one.clone(),
            _ => Value::Array(intents),
        };
        save(Path::new(path), &pyjson::dumps_indent1(&shown))?;
    }
    let output_arg = args.value("--output").expect("required");
    let output = Path::new(output_arg);
    let report = match written.as_slice() {
        [(intent, answers)] => api::write_file(step, intent, output, answers.as_ref()),
        _ => api::write_parts(step, &written, output),
    };
    let report = report.map_err(|e| match e {
        // An intent for another file, another loader's face numbering, a file or part that
        // cannot be loaded: Python's `ValueError`.
        Error::Load(_) | Error::Write(WriteError::Invalid(_)) | Error::Rules(_) => {
            Exit::new(2, format!("cannot write: {e}"))
        }
        Error::Write(_) | Error::Analysis(_) => {
            Exit::new(1, format!("write failed, {output_arg} left as it was: {e}"))
        }
    })?;
    print(out, &indent1_keeping_non_ascii(&report.to_dict()))
}

/// The rules' refusal as the command line words it: exit 2, `incomplete: ...` or `invalid
/// answer: ...` (a bad datum, a frame no longer offered, JSON that does not parse).
fn refused(e: Error) -> Exit {
    match e {
        Error::Rules(rules::Error::Incomplete(e)) => Exit::new(2, format!("incomplete: {e}")),
        Error::Rules(rules::Error::Invalid(m)) => Exit::new(2, format!("invalid answer: {m}")),
        other => Exit::error(other),
    }
}

/// `--part`, as an index of the file's parts. Python takes any integer and its loader refuses
/// a negative one by the same message as one past the end.
fn part_arg(step: &Path, args: &Args) -> Result<Option<usize>, Exit> {
    let Some(text) = args.value("--part") else {
        return Ok(None);
    };
    let value: i64 = text.trim().parse().expect("checked when parsed");
    match usize::try_from(value) {
        Ok(k) => Ok(Some(k)),
        Err(_) => {
            let count = part::parts(step).map_err(Exit::error)?.len();
            Err(Exit::error(format!(
                "{} has {count} part(s); there is no part {value}",
                step.display()
            )))
        }
    }
}

/// `open`: the file served once on 127.0.0.1 and the browser opened at the editor's page for
/// it (or, with `--no-browser`, only the page's address printed). Exit 2 for a name the
/// editor does not open or a bad `--site`; exit 1 when nothing fetched it in 10 minutes.
fn open(step: &Path, args: &Args, out: &mut impl Write) -> Result<(), Exit> {
    use crate::open::{Error as OpenError, Handoff, TIMEOUT, launch_browser};
    let failed = |e: OpenError| match e {
        OpenError::Refused(m) => Exit::new(2, format!("cannot open: {m}")),
        other => Exit::error(other),
    };
    let handoff =
        Handoff::new(step, args.value("--site"), args.flag("--editor")).map_err(failed)?;
    let page = handoff.page().to_string();
    print(out, &format!("Open {}: {page}", handoff.name()))?;
    out.flush().map_err(Exit::error)?;
    if !args.flag("--no-browser")
        && let Err(e) = launch_browser(&page)
    {
        eprintln!("warning: cannot open a browser ({e}); open the address above");
    }
    handoff.serve(TIMEOUT).map_err(failed)?;
    let target = page.split('?').next().unwrap_or(&page);
    print(out, &format!("Opened in {target}."))
}

/// `diff`: what changed from OLD to NEW, the answers carried to NEW, and NEW's picture.
fn diff(old: &Path, new: &Path, args: &Args, out: &mut impl Write) -> Result<(), Exit> {
    let part = part_arg(new, args)?;
    // The part is chosen in both files: say which file lacks it before either is loaded.
    if let Some(k) = part {
        for path in [old, new] {
            let count = part::parts(path).map_err(Exit::error)?.len();
            if k >= count {
                return Err(Exit::error(format!(
                    "{} has {count} part(s); there is no part {k}",
                    path.display()
                )));
            }
        }
    }
    let answers = match args.value("--answers") {
        Some(path) => answers_of(read_json(Path::new(path))?)?,
        None => Map::new(),
    };
    // With no part chosen, a file of several parts, or of a part placed other than once, is
    // compared as an assembly.
    let (before, after) = match part {
        Some(_) => {
            let load = |path: &Path| part::load(path, part).map_err(Exit::error);
            (load(old)?, load(new)?)
        }
        None => {
            let read = |path: &Path| part::File::read(path).map_err(Exit::error);
            let (old, new) = (read(old)?, read(new)?);
            if assembly::is_assembly(&old) || assembly::is_assembly(&new) {
                return diff_assembly(&old, &new, &answers, args, out);
            }
            let load = |file: &part::File| file.load(None).map_err(Exit::error);
            (load(&old)?, load(&new)?)
        }
    };
    let revised = crate::revision::diff_with_options(
        &before,
        &after,
        &answers,
        crate::revision::DiffOptions {
            scenes: args.flag("--pmi-scenes"),
        },
    )
    .map_err(refused)?;
    let dir = PathBuf::from(args.value("--output").expect("required"));
    std::fs::create_dir_all(&dir).map_err(|e| Exit::error(format!("{}: {e}", dir.display())))?;
    let title = args.value("--title").map(String::from).unwrap_or_else(|| {
        new.file_name()
            .map_or("part".into(), |n| n.to_string_lossy().into_owned())
    });
    let pretty = |v: &Value| indent1_keeping_non_ascii(v);
    let report = serde_json::to_value(&revised).map_err(Exit::error)?;
    save(
        &dir.join("answers.json"),
        &pretty(&Value::Object(revised.answers.clone())),
    )?;
    save(&dir.join("report.json"), &pretty(&report))?;
    save(
        &dir.join("report.md"),
        &revised.markdown(&title, Some("model.png")),
    )?;
    let png = crate::revision::picture(&before, &after, &revised, 900);
    let picture = dir.join("model.png");
    std::fs::write(&picture, png)
        .map_err(|e| Exit::error(format!("{}: {e}", picture.display())))?;
    print(
        out,
        &indent1_keeping_non_ascii(
            &json!({"verdict": revised.verdict, "summary": revised.summary}),
        ),
    )
}

/// `diff` of two revisions of an assembly, with no `--part`: every part compared, the parts
/// list and each changed part's report in `report.md`, the whole assembly's picture, and the
/// answers carried by part name.
fn diff_assembly(
    old: &part::File,
    new: &part::File,
    answers: &Map<String, Value>,
    args: &Args,
    out: &mut impl Write,
) -> Result<(), Exit> {
    let load = |file: &part::File| file.load_all().map_err(Exit::error);
    let (before, after) = (load(old)?, load(new)?);
    let compared = assembly::diff_with_options(
        &before,
        &after,
        answers,
        crate::revision::DiffOptions {
            scenes: args.flag("--pmi-scenes"),
        },
    )
    .map_err(refused)?;
    let dir = PathBuf::from(args.value("--output").expect("required"));
    std::fs::create_dir_all(&dir).map_err(|e| Exit::error(format!("{}: {e}", dir.display())))?;
    let title = args.value("--title").map(String::from).unwrap_or_else(|| {
        new.path()
            .file_name()
            .map_or("assembly".into(), |n| n.to_string_lossy().into_owned())
    });
    let pretty = |v: &Value| indent1_keeping_non_ascii(v);
    let report = serde_json::to_value(&compared).map_err(Exit::error)?;
    save(
        &dir.join("answers.json"),
        &pretty(&Value::Object(compared.answers())),
    )?;
    save(&dir.join("report.json"), &pretty(&report))?;
    save(
        &dir.join("report.md"),
        &compared.markdown(&title, Some("model.png"), args.value("--viewer")),
    )?;
    let png = compared.picture(&before, &after, 900);
    let picture = dir.join("model.png");
    std::fs::write(&picture, png)
        .map_err(|e| Exit::error(format!("{}: {e}", picture.display())))?;
    print(
        out,
        &indent1_keeping_non_ascii(
            &json!({"verdict": compared.verdict, "summary": compared.summary}),
        ),
    )
}

/// A JSON file, parsed. A file that cannot be read is an uncaught failure (exit 1); one that
/// is not JSON is refused as an invalid answer (exit 2), as Python's `JSONDecodeError` (a
/// `ValueError`) is.
fn read_json(path: &Path) -> Result<Value, Exit> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| Exit::error(format!("{}: {e}", path.display())))?;
    serde_json::from_str(&text).map_err(|e| Exit::new(2, format!("invalid answer: {e}")))
}

/// Answers must be a JSON object of question id to value.
fn answers_of(value: Value) -> Result<Map<String, Value>, Exit> {
    match value {
        Value::Object(map) => Ok(map),
        other => Err(Exit::new(
            2,
            format!("invalid answer: answers are a JSON object, not {other}"),
        )),
    }
}

/// `analyse`'s stdout: face count, features counted by family in the order families first
/// appear, and how many existing PMI items.
fn summary(analysis: &Value) -> Value {
    let mut counts = Map::new();
    for f in analysis["features"].as_array().into_iter().flatten() {
        let family = f["family"].as_str().unwrap_or_default().to_string();
        let n = counts.get(&family).and_then(Value::as_u64).unwrap_or(0);
        counts.insert(family, json!(n + 1));
    }
    let len = |key: &str| analysis[key].as_array().map_or(0, Vec::len);
    json!({"faces": len("faces"), "features": counts, "existing_pmi": len("existing")})
}

/// Writes `text` to `path` (no newline at the end, as Python's `write_text` of `json.dumps`).
fn save(path: &Path, text: &str) -> Result<(), Exit> {
    std::fs::write(path, text).map_err(|e| Exit::error(format!("{}: {e}", path.display())))
}

/// A line on stdout.
fn print(out: &mut impl Write, text: &str) -> Result<(), Exit> {
    writeln!(out, "{text}").map_err(Exit::error)
}

/// Python's `json.dumps(value, indent=1, ensure_ascii=False)`: [`pyjson::dumps_indent1`] with
/// every `\uXXXX` escape of a character from U+007F up written as the character itself
/// (surrogate pairs joined). The escapes `ensure_ascii=False` keeps (quotes, backslashes and
/// control characters) are left as they are.
fn indent1_keeping_non_ascii(value: &Value) -> String {
    let text = pyjson::dumps_indent1(value);
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(i) = rest.find('\\') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let hex = |s: &str| {
            s.get(2..6)
                .filter(|_| s.as_bytes().get(1) == Some(&b'u'))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
        };
        let (decoded, used) = match hex(rest) {
            Some(high @ 0xD800..=0xDBFF) => match hex(&rest[6..]) {
                Some(low @ 0xDC00..=0xDFFF) => (
                    char::from_u32(0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00)),
                    12,
                ),
                _ => (None, 6),
            },
            Some(code) if code >= 0x7F => (char::from_u32(code), 6),
            Some(_) => (None, 6),
            // `\"`, `\\`, `\n` and the like: the backslash and the character after it.
            None => (None, 2),
        };
        match decoded {
            Some(c) => out.push(c),
            None => out.push_str(&rest[..used]),
        }
        rest = &rest[used..];
    }
    out.push_str(rest);
    out
}
