//! `specify-core`: the command line (port of `cli.py`). [`main`] runs it on its arguments: the
//! binary (`main.rs`) and the Python package's `specify-core` console script both call it.
//!
//! ```text
//! specify-core parts STEP                        the file's parts, as JSON
//! specify-core bom STEP                          the assembly BOM, as JSON
//! specify-core analyse STEP -o ANALYSIS [--part K]
//! specify-core mesh STEP -o MESH [--part K] [--max-deviation-mm MM]
//! specify-core questions ANALYSIS [--answers ANSWERS] [--open]
//!                        [--resolve-defaults --step STEP [--accept-defaults FILE]]
//! specify-core write STEP ANALYSIS ANSWERS [ANALYSIS ANSWERS ...] -o OUT
//!                    [--accept-defaults] [--bom] [--intent INTENT]
//! specify-core diff OLD NEW -o DIR [--answers ANSWERS] [--part K] [--title TITLE]
//!                    [--viewer URL] [--pmi-scenes]
//!                    what changed between two revisions of a part, its answers carried
//!                    to the new one (revision.rs), or of an assembly, part by part
//!                    (revision/assembly.rs)
//! specify-core serve                             JSON-line requests on stdin (serve.rs)
//! specify-core mcp [--http [--port N]] [--allow DIR]...
//!                    a Model Context Protocol server on stdio or local HTTP (mcp.rs)
//! specify-core open STEP [--read-only | --in-place] [--port N] [--no-browser]
//!                    [--app DIR] [--wasm DIR]
//! specify-core open --diff OLD NEW [--answers FILE] [--in-place] [--port N] [--no-browser]
//!                    [--app DIR] [--wasm DIR]
//!                    the file (or NEW, compared with OLD) opened in the Specify app, served
//!                    on 127.0.0.1, its answers held by this command (session.rs) and saved
//!                    back to disk (open.rs)
//! specify-core completions SHELL                 a shell completion script (clap_complete)
//! ```
//!
//! Arguments are parsed by clap; help and usage errors are clap's (exit 0 and exit 2). A long
//! option may be shortened to any prefix only it has, a repeated option's last value wins and
//! `-h` gives a command's full help, as specify-core's argparse parser did.
//!
//! Outputs, exit codes and messages are specify-core's, so a host can run either engine:
//! `analyse` writes the analysis (Python's `json.dumps(indent=1)` text, non-ASCII escaped, so
//! the files compare byte for byte) and prints `{faces, features, existing_pmi}`; `mesh` writes
//! compact JSON and prints `{faces, triangles}`, warning on stderr of faces that did not
//! triangulate; `parts`, `questions` and `write` print indented JSON with non-ASCII kept. Exit 2
//! with `incomplete: ...`, `invalid answer: ...` or `cannot write: ...` on stderr (in an
//! assembly prefixed `part K: `), or with clap's usage error for bad arguments; exit 1 with
//! `write failed, OUT left as it was: ...` when a write does not verify.
//!
//! Where Python fails with an uncaught exception (a file that cannot be read or loaded, an
//! output that cannot be written) it exits 1 with a traceback; this exits 1 with `error:` and
//! the message on stderr. Python's second name for the program, `pmi-assist`, is an alias of the
//! Python package's entry point and has no counterpart for a Rust binary.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clap::error::ErrorKind;
use clap::{ArgAction, CommandFactory, FromArgMatches, Parser, Subcommand};

use crate::api::{self, Error};
use crate::part;
use crate::pyjson;
use crate::rules::{self, Intent};
use crate::serve;
use crate::writer::WriteError;
use serde_json::{Map, Value, json};

const PROG: &str = "specify-core";

// The command line. `-h` and `--help` both give the full help (`long_about`), as argparse's
// one help did; it is listed last, as clap lists its own.
#[derive(Parser)]
#[command(
    name = PROG,
    disable_help_flag = true,
    disable_help_subcommand = true,
    subcommand_required = true,
    infer_long_args = true,
    args_override_self = true
)]
struct Cli {
    #[arg(
        short = 'h',
        long = "help",
        action = ArgAction::HelpLong,
        global = true,
        display_order = 1000,
        help = "Print help"
    )]
    help: Option<bool>,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// list an assembly's parts
    Parts { step: PathBuf },
    /// recognise features and read existing PMI
    Analyse {
        step: PathBuf,
        /// analysis JSON (`-` for stdout; the summary then goes to stderr)
        #[arg(short, long)]
        output: PathBuf,
        /// which part of an assembly (see parts)
        #[arg(long, value_parser = part_number)]
        part: Option<i64>,
    },
    /// answer JSON-line requests on stdin (for a host process)
    Serve,
    /// a Model Context Protocol server for an LLM client (stdio, or local HTTP)
    #[command(long_about = "\
A Model Context Protocol server (revisions 2026-07-28 and 2025-11-25) for an LLM client such
as Claude Code or Claude Desktop: tools to open a part, answer its questions, review and
write its PMI. On stdio by default; with --http, Streamable HTTP on 127.0.0.1 with a random
bearer token printed at start. Open documents and their answers are held in memory and lost
when the server stops, unless a call names an answers_file. The show tool opens the Specify
page on the same answers, with the app and engine built into this command (nothing is
downloaded). Set SPECIFY_APP_DIR to use another app or SPECIFY_WASM_DIR to use a local
wasm-pack pkg/; a build made without the app needs SPECIFY_APP_DIR. See docs/mcp.md.")]
    Mcp {
        /// serve Streamable HTTP on 127.0.0.1 instead of stdio
        #[arg(long)]
        http: bool,
        /// the HTTP port (default: any free port, printed at start)
        #[arg(long, requires = "http")]
        port: Option<u16>,
        /// a directory calls may read and write in (repeatable; default the working directory)
        #[arg(long, value_name = "DIR", action = ArgAction::Append)]
        allow: Vec<PathBuf>,
    },
    /// triangles ranged by face index, for a picker
    Mesh {
        step: PathBuf,
        /// mesh JSON (`-` for stdout; the summary then goes to stderr)
        #[arg(short, long)]
        output: PathBuf,
        /// which part of an assembly (see parts)
        #[arg(long, value_parser = part_number)]
        part: Option<i64>,
        /// maximum surface/trim chord deviation in mm, at least 0.001; refuses unmet bounds. Exact on planes, spheres, line and circle trims; a sampled maximum on cylinders, cones, tori, B-splines and ellipse/B-spline/NURBS trims. Not a watertightness guarantee
        #[arg(long = "max-deviation-mm", allow_hyphen_values = true)]
        max_deviation_mm: Option<f64>,
    },
    /// list the questions, given answers so far
    Questions {
        analysis: PathBuf,
        /// answers JSON: {question id: value}
        #[arg(long)]
        answers: Option<PathBuf>,
        /// only unanswered questions
        #[arg(long)]
        open: bool,
        /// export the answers given, the defaults approved in --accept-defaults and face evidence as JSON; never every default, open questions stay open (listed on stderr)
        #[arg(long)]
        resolve_defaults: bool,
        /// matching STEP part for export (required with --resolve-defaults)
        #[arg(long)]
        step: Option<PathBuf>,
        /// with --resolve-defaults: JSON file of the defaults the person approved, by part, each with the value they saw: {"<part>": {"<id>": value}}. A default whose value has changed is refused
        #[arg(long, value_name = "FILE")]
        accept_defaults: Option<PathBuf>,
        /// add each default's source: the rule behind it (docs/rule-sources.md)
        #[arg(long)]
        sources: bool,
    },
    /// write intent as AP242 PMI
    Write {
        step: PathBuf,
        /// analysis and answers JSON; in an assembly, a pair for each part to write
        #[arg(required = true, num_args = 1.., value_name = "ANALYSIS ANSWERS")]
        pairs: Vec<String>,
        #[arg(short, long)]
        output: String,
        /// take PMI defaults for the rest
        #[arg(long)]
        accept_defaults: bool,
        /// explicitly accept optional BOM suggestions
        #[arg(long)]
        bom: bool,
        /// also write the intent JSON here
        #[arg(long)]
        intent: Option<PathBuf>,
    },
    /// what changed between two revisions of a part or an assembly
    Diff {
        /// the STEP file as it was (a part or an assembly)
        old: PathBuf,
        /// the STEP file as it is now
        new: PathBuf,
        /// a directory: answers.json, report.json, report.md, model.png; an assembly's
        /// answers.json is by part
        #[arg(short, long)]
        output: PathBuf,
        /// answers JSON for the old part: {question id: value}; for an assembly, {part name:
        /// {question id: value}}
        #[arg(long)]
        answers: Option<PathBuf>,
        /// compare one part of an assembly (see parts)
        #[arg(long, value_parser = part_number)]
        part: Option<i64>,
        /// the report's title (the new file's name)
        #[arg(long)]
        title: Option<String>,
        /// the /diff page's link for these revisions: an assembly's changed parts link to it
        /// with &part=NAME
        #[arg(long)]
        viewer: Option<String>,
        /// include the compared PMI scenes in report.json
        #[arg(long)]
        pmi_scenes: bool,
    },
    /// open a STEP file in Specify in your browser, served from this computer
    #[command(long_about = "\
Open a STEP file in Specify in your browser, served by this command on 127.0.0.1: nothing
is uploaded. Specify's Save writes the specified file, from the answers given in the page,
next to the input as <name>.specified.step (then -2, -3, ...; later saves update the copy
this run wrote, and no other file is overwritten), or with
--in-place over the input (through a link, to its target), the original kept once as .bak.
With --diff OLD NEW the page shows what changed from OLD and starts from the answers carried
to NEW (as `specify-core diff` carries them); Save writes NEW. Stops 90 seconds after the last
page is closed, or after 30 minutes without a request.

The app and its engine are built into this command: nothing is downloaded. Use --app DIR (or
SPECIFY_APP_DIR) for another app and --wasm DIR (or SPECIFY_WASM_DIR) for a local wasm-pack
pkg/ directory (else an app's own wasm/, else the built-in engine). A build made without the
app needs --app.")]
    Open {
        /// a .step or .stp file, or either gzipped
        #[arg(required_unless_present = "diff", conflicts_with = "diff")]
        step: Option<PathBuf>,
        /// compare OLD to NEW, and specify NEW
        #[arg(long, num_args = 2, value_names = ["OLD", "NEW"])]
        diff: Option<Vec<PathBuf>>,
        /// with --diff: the old answers (default: those stored in OLD)
        #[arg(long, value_name = "FILE", requires = "diff")]
        answers: Option<PathBuf>,
        /// refuse answer changes and saves
        #[arg(long, conflicts_with_all = ["in_place", "diff"])]
        read_only: bool,
        /// save over the input, keeping the original once as .bak
        #[arg(long)]
        in_place: bool,
        /// the port on 127.0.0.1 (default: any free port)
        #[arg(long)]
        port: Option<u16>,
        /// print the address to open instead of opening a browser
        #[arg(long)]
        no_browser: bool,
        /// a local Specify app (index.html at its root); overrides SPECIFY_APP_DIR and the built-in app
        #[arg(long, value_name = "DIR")]
        app: Option<PathBuf>,
        /// a local wasm-pack pkg/ directory; overrides SPECIFY_WASM_DIR and the built-in engine
        #[arg(long, value_name = "DIR")]
        wasm: Option<PathBuf>,
    },
    /// print the assembly BOM as JSON
    Bom { step: PathBuf },
    /// print a shell completion script to stdout
    Completions {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

/// `--part`: an integer, surrounding blanks allowed (as Python's `int`). A negative one is
/// refused when the file is read, as Python's loader refuses it.
fn part_number(text: &str) -> Result<i64, std::num::ParseIntError> {
    text.trim().parse()
}

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

    /// An uncaught failure (Python's traceback, exit 1).
    fn error(message: impl std::fmt::Display) -> Self {
        Exit::new(1, format!("error: {message}"))
    }
}

/// The command line's clap command. Every command takes a value that looks like a negative
/// number (`--part -1`, `--title -5`, `parts -1`) as a value, as argparse did, rather than as
/// an unknown option; other values starting with `-` stay unknown options.
fn command() -> clap::Command {
    Cli::command().mut_subcommands(|c| c.allow_negative_numbers(true))
}

/// The arguments parsed, or clap's error (help included, exit 0) to print.
fn parse(argv: &[String]) -> Result<Cmd, clap::Error> {
    let mut matches = command()
        .try_get_matches_from(std::iter::once(PROG).chain(argv.iter().map(String::as_str)))?;
    let cli = Cli::from_arg_matches_mut(&mut matches)?;
    if let Cmd::Write { pairs, .. } = &cli.command
        && !pairs.len().is_multiple_of(2)
    {
        let mut parser = command();
        parser.build();
        let write = parser.find_subcommand_mut("write").expect("a command");
        return Err(write.error(
            ErrorKind::WrongNumberOfValues,
            "write takes an analysis and its answers for each part",
        ));
    }
    Ok(cli.command)
}

/// Runs the command line on `argv` (the arguments after the program's name) and returns the
/// exit code; replies go to stdout, messages to stderr.
pub fn main(argv: &[String]) -> u8 {
    let command = match parse(argv) {
        Ok(command) => command,
        Err(e) => {
            let _ = e.print();
            return u8::try_from(e.exit_code()).unwrap_or(2);
        }
    };
    let mut stdout = io::stdout().lock();
    let result = run(command, &mut stdout);
    let _ = stdout.flush();
    match result {
        Ok(()) => 0,
        Err(exit) => {
            let _ = writeln!(io::stderr(), "{}", exit.message);
            exit.code
        }
    }
}

fn run(cmd: Cmd, out: &mut impl Write) -> Result<(), Exit> {
    match cmd {
        Cmd::Serve => {
            let stdin = io::stdin().lock();
            serve::serve(stdin, out).map_err(Exit::error)
        }
        Cmd::Mcp { http, port, allow } => mcp(http, port, &allow, out),
        Cmd::Completions { shell } => {
            let mut script = Vec::new();
            clap_complete::generate(shell, &mut command(), PROG, &mut script);
            if shell == clap_complete::Shell::Bash {
                // clap_complete 4.6 names the subcommands' cases with the program name's `-`
                // escaped as a subcommand separator (`specify__subcmd__core__subcmd__write`) but
                // selects them as `specify__core__subcmd__write`, so no subcommand's options
                // would complete.
                let text = String::from_utf8(script).map_err(Exit::error)?;
                script = text
                    .replace("specify__subcmd__core__subcmd__", "specify__core__subcmd__")
                    .into_bytes();
            }
            out.write_all(&script).map_err(Exit::error)
        }
        Cmd::Bom { step } => {
            let rows = crate::bom::read_path(&step).map_err(Exit::error)?;
            print(
                out,
                &serde_json::to_string_pretty(&rows).map_err(Exit::error)?,
            )
        }
        Cmd::Parts { step } => {
            let parts = api::parts(&step).map_err(Exit::error)?;
            print(out, &indent1_keeping_non_ascii(&Value::Array(parts)))
        }
        Cmd::Analyse { step, output, part } => {
            let part = part_arg(&step, part)?;
            // The file is read once, for the part and the other parts its mates need.
            let file = part::File::read(&step).map_err(Exit::error)?;
            let loaded = file.load(part).map_err(Exit::error)?;
            let (analysis, warnings) =
                api::analyse_reporting_in(&file, &loaded).map_err(Exit::error)?;
            emit(
                out,
                &output,
                &pyjson::dumps_indent1(&analysis),
                &pyjson::dumps(&summary(&analysis)),
            )?;
            for line in warnings {
                eprintln!("warning: {line}");
            }
            Ok(())
        }
        Cmd::Mesh {
            step,
            output,
            part,
            max_deviation_mm,
        } => {
            let part = part_arg(&step, part)?;
            let mesh = api::mesh_file_with_max_deviation(&step, part, max_deviation_mm)
                .map_err(Exit::error)?;
            let count = |key: &str| mesh[key].as_array().map_or(0, Vec::len);
            let shown = json!({"faces": count("faces"), "triangles": count("indices") / 3});
            emit(
                out,
                &output,
                &pyjson::dumps_compact(&mesh),
                &pyjson::dumps(&shown),
            )?;
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
        Cmd::Questions {
            analysis,
            answers,
            open,
            resolve_defaults,
            step,
            accept_defaults,
            sources,
        } => {
            let analysis = read_json(&analysis)?;
            let answers = match answers {
                Some(path) => answers_of(read_json(&path)?)?,
                None => Map::new(),
            };
            if resolve_defaults {
                if open {
                    return Err(Exit::new(
                        2,
                        "--open cannot be combined with --resolve-defaults",
                    ));
                }
                let step = step.ok_or_else(|| {
                    Exit::new(
                        2,
                        "--resolve-defaults requires --step STEP for face evidence",
                    )
                })?;
                let index = analysis["binding"]["part"].as_u64().unwrap_or(0) as usize;
                let loaded = part::load(&step, Some(index)).map_err(Exit::error)?;
                let approved = match accept_defaults {
                    Some(path) => api::approvals(&read_json(&path)?).ok_or_else(|| {
                        Exit::new(2, format!("--accept-defaults: {}", api::APPROVALS_FORM))
                    })?,
                    None => api::Approved::new(),
                };
                let exported = api::export_answers(&loaded, &analysis, &answers, &approved)
                    .map_err(|e| Exit::new(2, e.to_string()))?;
                if !exported.complete() {
                    eprintln!("open: {}", exported.missing.join(", "));
                }
                return print(
                    out,
                    &serde_json::to_string_pretty(&exported.answers).map_err(Exit::error)?,
                );
            }
            if step.is_some() {
                return Err(Exit::new(2, "--step requires --resolve-defaults"));
            }
            if accept_defaults.is_some() {
                return Err(Exit::new(
                    2,
                    "--accept-defaults requires --resolve-defaults",
                ));
            }
            let qs = api::questions(&analysis, &answers).map_err(refused)?;
            let shown: Vec<Value> = if open {
                rules::open_questions(&qs, &answers)
                    .iter()
                    .map(|q| q.to_dict_with(sources))
                    .collect()
            } else {
                qs.iter().map(|q| q.to_dict_with(sources)).collect()
            };
            print(out, &indent1_keeping_non_ascii(&Value::Array(shown)))
        }
        Cmd::Write {
            step,
            pairs,
            output,
            accept_defaults,
            bom,
            intent,
        } => write(
            &step,
            &pairs,
            &output,
            accept_defaults,
            bom,
            intent.as_deref(),
            out,
        ),
        Cmd::Diff {
            old,
            new,
            output,
            answers,
            part,
            title,
            viewer,
            pmi_scenes,
        } => diff(
            &old,
            &new,
            &Compared {
                output,
                answers,
                part,
                title,
                viewer,
                scenes: pmi_scenes,
            },
            out,
        ),
        Cmd::Open {
            step,
            diff,
            answers,
            read_only,
            in_place,
            port,
            no_browser,
            app,
            wasm,
        } => {
            let options = crate::open::Options {
                read_only,
                port: port.unwrap_or(0),
                part: None,
            };
            if answers.is_some() && diff.is_none() {
                return Err(Exit::new(
                    2,
                    "cannot open: --answers gives the old answers of --diff OLD NEW",
                ));
            }
            let opening = match (step, diff.as_deref()) {
                (Some(step), _) => Opening::File(step),
                (None, Some([old, new])) => Opening::Diff {
                    old: old.clone(),
                    new: new.clone(),
                    answers,
                },
                _ => return Err(Exit::new(2, "open FILE, or open --diff OLD NEW")),
            };

            open(
                &opening,
                in_place,
                &options,
                app.as_deref(),
                wasm.as_deref(),
                no_browser,
                out,
            )
        }
    }
}

/// `write`: each pair's intent applied (refusals naming the part in an assembly), the intents
/// saved when asked, then written: one part on its own, several together.
fn write(
    step: &Path,
    pairs: &[String],
    output_arg: &str,
    accept_defaults: bool,
    bom: bool,
    intent_path: Option<&Path>,
    out: &mut impl Write,
) -> Result<(), Exit> {
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
        let mut intent = match api::apply(&analysis, &answers, accept_defaults) {
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
        if bom {
            crate::bom::accept_suggestions(&analysis, &answers, &mut intent)
                .map_err(Exit::error)?;
        }
        // Every default taken: those that write no PMI are stored with the answers (ADR P9).
        // A part that cannot be loaded or planned stores what was given; the write refuses it.
        let answers = if accept_defaults {
            let index = analysis["binding"]["part"].as_u64().unwrap_or(0) as usize;
            part::load(step, Some(index))
                .ok()
                .and_then(|loaded| api::stored_with_defaults(&loaded, &analysis, &answers).ok())
                .unwrap_or(answers)
        } else {
            answers
        };
        written.push((intent, Some(answers)));
    }
    if let Some(path) = intent_path {
        let intents: Vec<Value> = written.iter().map(|(i, _)| i.to_dict()).collect();
        let shown = match intents.as_slice() {
            [one] => one.clone(),
            _ => Value::Array(intents),
        };
        save(path, &pyjson::dumps_indent1(&shown))?;
    }
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
        Error::Unapproved(_) => Exit::new(2, format!("cannot write: {e}")),
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
fn part_arg(step: &Path, part: Option<i64>) -> Result<Option<usize>, Exit> {
    let Some(value) = part else {
        return Ok(None);
    };
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

/// What `open` opens.
enum Opening {
    File(PathBuf),
    Diff {
        old: PathBuf,
        new: PathBuf,
        answers: Option<PathBuf>,
    },
}

/// `open`: a session of the file (or of the pair, compared as `diff` compares them), the
/// Specify app served on 127.0.0.1 for it and the browser opened at it (or, with
/// `--no-browser`, only its address printed); then each save's notice and the stop's. Exit 2
/// for a name Specify does not open, no app, or options that cannot be combined.
fn open(
    opening: &Opening,
    in_place: bool,
    options: &crate::open::Options,
    app: Option<&Path>,
    wasm: Option<&Path>,
    no_browser: bool,
    out: &mut impl Write,
) -> Result<(), Exit> {
    use crate::open::{App, CLOSE_DELAY, Error as OpenError, IDLE, assets, launch_browser};
    use crate::session::{Opened, Session};
    let failed = |e: OpenError| match e {
        OpenError::Refused(m) => Exit::new(2, format!("cannot open: {m}")),
        other => Exit::error(other),
    };
    let named = |path: &Path| {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if crate::open::is_step_name(&name) {
            Ok(())
        } else {
            Err(Exit::new(
                2,
                format!(
                    "cannot open: {} is not a STEP file: Specify opens .step or .stp, or either \
                     gzipped",
                    path.display()
                ),
            ))
        }
    };
    let session = match opening {
        Opening::File(step) => {
            named(step)?;
            Session::open(Opened::read(step).map_err(Exit::error)?, in_place)
        }
        Opening::Diff { old, new, answers } => {
            named(old)?;
            named(new)?;
            let given = match answers {
                Some(path) => answers_of(read_json(path)?)?,
                None => Map::new(),
            };
            let old = Opened::read(old).map_err(Exit::error)?;
            let new = Opened::read(new).map_err(Exit::error)?;
            Session::diff(old, new, None, &given, in_place)
        }
    }
    .map_err(refused)?;
    let found = assets(app, wasm).map_err(failed)?;
    let session = std::sync::Arc::new(std::sync::Mutex::new(session));
    let served = App::new(session, found, options).map_err(failed)?;
    let url = served.url();
    print(out, &format!("Open {}: {url}", served.name()))?;
    out.flush().map_err(Exit::error)?;
    if !no_browser && let Err(e) = launch_browser(&url) {
        let _ = writeln!(
            io::stderr(),
            "warning: cannot open a browser ({e}); open the address above"
        );
    }
    let mut written = Ok(());
    served
        .serve(CLOSE_DELAY, IDLE, |notice| {
            if written.is_ok() {
                written = writeln!(out, "{notice}").and_then(|_| out.flush());
            }
        })
        .map_err(failed)?;
    written.map_err(Exit::error)
}

/// `mcp`: the MCP server on stdio, or on HTTP with its address and token printed first.
fn mcp(http: bool, port: Option<u16>, allow: &[PathBuf], out: &mut impl Write) -> Result<(), Exit> {
    use crate::mcp;
    let server = mcp::Server::new(allow).map_err(|e| Exit::new(2, format!("cannot serve: {e}")))?;
    if !http {
        let stdin = io::stdin().lock();
        let mut server = server;
        return mcp::serve_stdio(stdin, out, &mut server).map_err(Exit::error);
    }
    let listener = mcp::bind(port.unwrap_or(0)).map_err(Exit::error)?;
    let bound = listener.local_addr().map_err(Exit::error)?.port();
    let token = mcp::new_token().map_err(Exit::error)?;
    let dirs: Vec<String> = server
        .allowed()
        .iter()
        .map(|d| d.display().to_string())
        .collect();
    print(
        out,
        &format!("MCP endpoint: http://127.0.0.1:{bound}{}", mcp::ENDPOINT),
    )?;
    print(out, &format!("Authorization: Bearer {token}"))?;
    print(out, &format!("Allowed directories: {}", dirs.join(", ")))?;
    out.flush().map_err(Exit::error)?;
    mcp::serve_http(listener, token, server).map_err(Exit::error)
}

/// `diff`'s options.
struct Compared {
    output: PathBuf,
    answers: Option<PathBuf>,
    part: Option<i64>,
    title: Option<String>,
    viewer: Option<String>,
    scenes: bool,
}

/// `diff`: what changed from OLD to NEW, the answers carried to NEW, and NEW's picture.
fn diff(old: &Path, new: &Path, args: &Compared, out: &mut impl Write) -> Result<(), Exit> {
    let part = part_arg(new, args.part)?;
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
    let answers = match &args.answers {
        Some(path) => answers_of(read_json(path)?)?,
        None => Map::new(),
    };
    // With no part chosen, a file of several parts, or of a part placed other than once, is
    // compared as an assembly.
    let read = |path: &Path| part::File::read(path).map_err(Exit::error);
    let compared = crate::revision::compare(
        &read(old)?,
        &read(new)?,
        part,
        &answers,
        crate::revision::DiffOptions {
            scenes: args.scenes,
        },
    )
    .map_err(refused)?;
    let dir = &args.output;
    std::fs::create_dir_all(dir).map_err(|e| Exit::error(format!("{}: {e}", dir.display())))?;
    let pretty = |v: &Value| indent1_keeping_non_ascii(v);
    save(
        &dir.join("answers.json"),
        &pretty(&Value::Object(compared.answers())),
    )?;
    save(&dir.join("report.json"), &pretty(&compared.report()))?;
    let (markdown, png) = match &compared {
        crate::revision::Comparison::Part {
            before,
            after,
            revised,
        } => {
            let title = args.title.clone().unwrap_or_else(|| {
                new.file_name()
                    .map_or("part".into(), |n| n.to_string_lossy().into_owned())
            });
            (
                revised.markdown(&title, Some("model.png")),
                crate::revision::picture(before, after, revised, 900),
            )
        }
        crate::revision::Comparison::Assembly {
            before,
            after,
            compared,
        } => {
            let title = args.title.clone().unwrap_or_else(|| {
                new.file_name()
                    .map_or("assembly".into(), |n| n.to_string_lossy().into_owned())
            });
            (
                compared.markdown(&title, Some("model.png"), args.viewer.as_deref()),
                compared.picture(before, after, 900),
            )
        }
    };
    save(&dir.join("report.md"), &markdown)?;
    let picture = dir.join("model.png");
    std::fs::write(&picture, png)
        .map_err(|e| Exit::error(format!("{}: {e}", picture.display())))?;
    print(
        out,
        &indent1_keeping_non_ascii(
            &json!({"verdict": compared.verdict(), "summary": compared.summary()}),
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

/// Writes a command's JSON file and its one-line summary. To a file, the summary goes to stdout;
/// to `-` the JSON goes to stdout and the summary to stderr, so the JSON can be piped into
/// another program (`specify-core analyse part.step -o - | jq .features`).
fn emit(out: &mut impl Write, path: &Path, text: &str, shown: &str) -> Result<(), Exit> {
    if path.as_os_str() == "-" {
        print(out, text)?;
        eprintln!("{shown}");
        Ok(())
    } else {
        save(path, text)?;
        print(out, shown)
    }
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
