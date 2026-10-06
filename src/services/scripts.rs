//! Daemon-owned programs whose output feeds gadgets: a `[Custom]`'s
//! `exec`. A gadget describes what to run as a [`Spec`]
//! (`Gadget::script`); the daemon runs every distinct spec once,
//! however many panels show it (two monitors don't run `checkupdates`
//! twice, which it doesn't even tolerate), keeps the last [`Output`]
//! per spec, and hands them to gadgets read-only through
//! `gadget::Context`. A gadget wanting a fresh run now returns
//! `Action::Script(Command::Refresh(spec))`.
//!
//! Programs are run as written, without a shell (see
//! [`crate::process`]).

use std::collections::HashMap;
use std::process::Stdio;
use std::time::Duration;

use iced::Subscription;
use iced::futures::stream;

/// A program to run, and how to read it. Its identity: two gadgets
/// with the same spec share one run.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Spec {
    /// The program and its arguments.
    pub argv: Vec<String>,
    /// Seconds between runs; 0 runs once (and on `Refresh`).
    pub interval: u64,
    pub return_type: ReturnType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReturnType {
    /// The output, trimmed, is the text.
    Text,
    /// A JSON object, waybar's shape: `{"text": .., "class": .. }`, plus
    /// our `icon`. Other fields are ignored.
    Json,
}

/// What a run said. A failed run (couldn't start, non-zero exit, JSON
/// expected and not given) is logged and gives an empty text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Output {
    pub text: String,
    /// An icon name replacing the gadget's.
    pub icon: Option<String>,
    /// CSS classes for the gadget's button.
    pub classes: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum Event {
    Ran(Spec, Output),
}

#[derive(Debug, Clone)]
pub enum Command {
    /// Run the spec again now.
    Refresh(Spec),
}

#[derive(Default)]
pub struct Scripts {
    outputs: HashMap<Spec, Output>,
    /// Bumped by `Refresh`: part of the subscription's identity, so
    /// it restarts and runs right away.
    generations: HashMap<Spec, u64>,
}

impl Scripts {
    /// The last output of `spec`, `None` before its first run.
    pub fn output(&self, spec: &Spec) -> Option<&Output> {
        self.outputs.get(spec)
    }

    /// Icon names the outputs mention, for the daemon to resolve.
    pub fn icon_names(&self) -> impl Iterator<Item = &str> {
        self.outputs.values().filter_map(|o| o.icon.as_deref())
    }

    pub fn apply(&mut self, event: Event) {
        match event {
            Event::Ran(spec, output) => {
                self.outputs.insert(spec, output);
            }
        }
    }

    pub fn run(&mut self, command: Command) {
        match command {
            Command::Refresh(spec) => *self.generations.entry(spec).or_default() += 1,
        }
    }

    /// One runner per distinct spec among `specs` (what the panels'
    /// gadgets ask for now).
    pub fn subscription(&self, specs: impl IntoIterator<Item = Spec>) -> Subscription<Event> {
        let mut seen = Vec::new();
        let runners = specs.into_iter().filter_map(|spec| {
            if seen.contains(&spec) {
                return None;
            }
            seen.push(spec.clone());
            let generation = self.generations.get(&spec).copied().unwrap_or(0);
            Some(Subscription::run_with((spec, generation), |(spec, _)| {
                runs(spec.clone())
            }))
        });
        Subscription::batch(runners)
    }
}

/// Runs the spec now and then every `interval` seconds (once, when
/// 0). Dropping the stream (the gadget went away, or asked for a fresh
/// run) kills a running program.
fn runs(spec: Spec) -> impl stream::Stream<Item = Event> {
    stream::unfold(Some(false), move |state| {
        let spec = spec.clone();
        async move {
            let waited = state?;
            if waited {
                tokio::time::sleep(Duration::from_secs(spec.interval)).await;
            }
            let output = match capture(&spec.argv).await {
                Ok(raw) => parse(&spec, &raw),
                Err(e) => {
                    log::warn!("{}: {e}", spec.argv.join(" "));
                    Output::default()
                }
            };
            let next = (spec.interval > 0).then_some(true);
            Some((Event::Ran(spec, output), next))
        }
    })
}

/// The program's standard output; stderr goes through to ours.
async fn capture(argv: &[String]) -> Result<String, String> {
    let (program, args) = argv.split_first().ok_or("empty command")?;
    let output = tokio::process::Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| format!("can't run {program:?}: {e}"))?;
    if !output.status.success() {
        return Err(format!("{program:?} exited with {}", output.status));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn parse(spec: &Spec, raw: &str) -> Output {
    let raw = raw.trim();
    let text = || Output {
        text: raw.to_owned(),
        ..Output::default()
    };
    match spec.return_type {
        ReturnType::Text => text(),
        ReturnType::Json => parse_json(raw).unwrap_or_else(|e| {
            log::warn!("{}: not the JSON expected: {e}", spec.argv.join(" "));
            text()
        }),
    }
}

/// `text`, `class` (a string or an array of them) and `icon`.
fn parse_json(raw: &str) -> Result<Output, String> {
    if raw.is_empty() {
        return Ok(Output::default());
    }
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    let object = value.as_object().ok_or("not an object")?;
    let string = |key: &str| object.get(key).and_then(|v| v.as_str()).map(str::to_owned);
    let classes = match object.get("class") {
        None | Some(serde_json::Value::Null) => Vec::new(),
        Some(serde_json::Value::String(s)) => s.split_whitespace().map(str::to_owned).collect(),
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str())
            .map(str::to_owned)
            .collect(),
        Some(other) => return Err(format!("class is {other}, not a string or an array")),
    };
    Ok(Output {
        text: string("text").unwrap_or_default(),
        icon: string("icon").filter(|s| !s.is_empty()),
        classes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(return_type: ReturnType) -> Spec {
        Spec {
            argv: vec!["x".into()],
            interval: 0,
            return_type,
        }
    }

    #[test]
    fn json_output() {
        assert_eq!(
            parse_json(r#"{"text": "3", "class": "warning urgent", "icon": "x", "tooltip": "t"}"#)
                .unwrap(),
            Output {
                text: "3".into(),
                icon: Some("x".into()),
                classes: vec!["warning".into(), "urgent".into()],
            }
        );
        assert_eq!(
            parse_json(r#"{"class": ["a", "b"]}"#).unwrap().classes,
            ["a", "b"]
        );
        assert_eq!(parse_json("").unwrap(), Output::default());
        assert!(parse_json("[1]").is_err());
        assert!(parse_json(r#"{"class": 3}"#).is_err());
    }

    #[test]
    fn text_and_bad_json_are_trimmed_text() {
        assert_eq!(parse(&spec(ReturnType::Text), " 3 \n").text, "3");
        assert_eq!(parse(&spec(ReturnType::Json), "nope\n").text, "nope");
    }

    #[test]
    fn refresh_bumps_the_generation() {
        let mut s = Scripts::default();
        s.run(Command::Refresh(spec(ReturnType::Text)));
        s.run(Command::Refresh(spec(ReturnType::Text)));
        assert_eq!(s.generations[&spec(ReturnType::Text)], 2);
        assert!(s.output(&spec(ReturnType::Text)).is_none());
        s.apply(Event::Ran(spec(ReturnType::Text), Output::default()));
        assert_eq!(s.output(&spec(ReturnType::Text)), Some(&Output::default()));
    }
}
