pub const HELP: &str = "\
Fetch the ONNX Runtime build ort-sys links into ort-sys's own cache, so a
build without network finds it there.

Usage: ort-cache [OPTIONS]

Options:
      --target <TARGET>  The target to fetch for [default: the host's]
      --check            Only print the folder; exit 1 when it lacks the library
  -h, --help             Print help

It prints the folder ort-sys reads for the target. The archive is pinned by
ort-sys's own table and its sha256 is checked before anything is extracted.
";

const TARGET: &str = "--target <TARGET>";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub target: Option<String>,
    pub check: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Parsed {
    Run(Options),
    Help,
    Usage(UsageError),
}

#[derive(Debug, PartialEq, Eq)]
pub enum UsageError {
    Unexpected(String),
    Repeated(&'static str),
    MissingValue,
}

pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Parsed {
    let mut opts = Options::default();
    let mut target_seen = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let value = match arg.as_str() {
            "-h" | "--help" => return Parsed::Help,
            "--check" => {
                if opts.check {
                    return Parsed::Usage(UsageError::Repeated("--check"));
                }
                opts.check = true;
                continue;
            }
            "--target" => {
                if target_seen {
                    return Parsed::Usage(UsageError::Repeated(TARGET));
                }
                args.next().filter(|v| !v.starts_with('-'))
            }
            other => match other.strip_prefix("--target=") {
                Some(_) if target_seen => return Parsed::Usage(UsageError::Repeated(TARGET)),
                Some(v) => Some(v.to_string()),
                None => return Parsed::Usage(UsageError::Unexpected(other.to_string())),
            },
        };
        target_seen = true;
        match value.filter(|v| !v.is_empty()) {
            Some(v) => opts.target = Some(v),
            None => return Parsed::Usage(UsageError::MissingValue),
        }
    }
    Parsed::Run(opts)
}

impl UsageError {
    pub fn render(&self, color: bool) -> String {
        let paint = |code: &str, text: &str| {
            if color {
                format!("\x1b[{code}m{text}\x1b[0m")
            } else {
                text.to_string()
            }
        };
        let error = paint("1;31", "error:");
        let (first, usage) = match self {
            UsageError::Unexpected(arg) => (
                format!("{error} unexpected argument '{}' found", paint("33", arg)),
                true,
            ),
            UsageError::Repeated(arg) => (
                format!(
                    "{error} the argument '{}' cannot be used multiple times",
                    paint("33", arg)
                ),
                true,
            ),
            UsageError::MissingValue => (
                format!(
                    "{error} a value is required for '{}' but none was supplied",
                    paint("33", TARGET)
                ),
                false,
            ),
        };
        let mut out = format!("{first}\n\n");
        if usage {
            out.push_str(&format!(
                "{} {} [OPTIONS]\n\n",
                paint("1;4", "Usage:"),
                paint("1", "ort-cache")
            ));
        }
        out.push_str(&format!(
            "For more information, try '{}'.\n",
            paint("1", "--help")
        ));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(args: &[&str]) -> Parsed {
        parse(args.iter().map(|a| a.to_string()))
    }

    fn target(t: &str, check: bool) -> Parsed {
        Parsed::Run(Options {
            target: Some(t.into()),
            check,
        })
    }

    #[test]
    fn good_arguments_parse_in_any_order_and_either_target_spelling() {
        assert_eq!(parsed(&[]), Parsed::Run(Options::default()));
        assert_eq!(parsed(&["--target", "a", "--check"]), target("a", true));
        assert_eq!(parsed(&["--check", "--target=b"]), target("b", true));
        assert_eq!(parsed(&["--target=-x"]), target("-x", false));
    }

    #[test]
    fn help_wins_until_a_bad_argument_comes_first() {
        assert_eq!(parsed(&["-h"]), Parsed::Help);
        assert_eq!(parsed(&["--check", "--help", "--bogus"]), Parsed::Help);
        assert_eq!(
            parsed(&["--bogus", "--help"]),
            Parsed::Usage(UsageError::Unexpected("--bogus".into()))
        );
    }

    #[test]
    fn repeats_missing_values_and_strays_are_usage_errors() {
        use UsageError::*;
        for (args, err) in [
            (&["stray"][..], Unexpected("stray".into())),
            (&["--check", "--check"], Repeated("--check")),
            (&["--target", "a", "--target=b"], Repeated(TARGET)),
            (&["--target=a", "--target", "b"], Repeated(TARGET)),
            (&["--target"], MissingValue),
            (&["--target", "--check"], MissingValue),
            (&["--target", ""], MissingValue),
            (&["--target="], MissingValue),
        ] {
            assert_eq!(parsed(args), Parsed::Usage(err), "{args:?}");
        }
    }

    #[test]
    fn usage_errors_render_in_clap_shape_plain_and_coloured() {
        let shape = |first: &str| {
            format!(
                "{first}\n\nUsage: ort-cache [OPTIONS]\n\nFor more information, try '--help'.\n"
            )
        };
        assert_eq!(
            UsageError::Unexpected("x".into()).render(false),
            shape("error: unexpected argument 'x' found")
        );
        assert_eq!(
            UsageError::Repeated("--check").render(false),
            shape("error: the argument '--check' cannot be used multiple times")
        );
        assert_eq!(
            UsageError::MissingValue.render(false),
            "error: a value is required for '--target <TARGET>' but none was supplied\n\nFor more information, try '--help'.\n"
        );
        let coloured = UsageError::Unexpected("x".into()).render(true);
        assert!(
            coloured.starts_with("\x1b[1;31merror:\x1b[0m unexpected argument '\x1b[33mx\x1b[0m'")
        );
        assert!(coloured.contains("\x1b[1;4mUsage:\x1b[0m \x1b[1mort-cache\x1b[0m [OPTIONS]"));
        assert!(coloured.contains("try '\x1b[1m--help\x1b[0m'."));
    }
}
