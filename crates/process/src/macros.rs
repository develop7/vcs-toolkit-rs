//! Declarative macros shared by the CLI wrappers, so the identical scaffolding
//! is written once here instead of copy-pasted into `vcs-git`/`vcs-jj`/`vcs-github`.

/// Emit the standard wrapper scaffolding for a client whose sole field is
/// `core: `[`CliClient`](crate::CliClient)`<R>`: the `struct`, `new`, `Default`,
/// `with_runner`, and `default_timeout`. The wrapper then only writes its
/// object-safe `*Api` trait and the typed command methods (which call
/// `self.core.exec_in(...)` / `run_text` / `parsed` / …).
///
/// `$binary` is the CLI program name (typically the wrapper's `const BINARY`).
/// A leading doc comment / attributes are forwarded onto the generated struct.
///
/// ```ignore
/// vcs_process::cli_client!(
///     /// The real Git client. Generic over the [`Runner`].
///     pub struct Git => BINARY
/// );
/// ```
#[macro_export]
macro_rules! cli_client {
    ($(#[$meta:meta])* $vis:vis struct $name:ident => $binary:expr) => {
        $(#[$meta])*
        $vis struct $name<R: $crate::Runner = $crate::JobRunner> {
            core: $crate::CliClient<R>,
        }

        impl $name<$crate::JobRunner> {
            /// A client backed by the real CLI binary.
            $vis fn new() -> Self {
                $name {
                    core: $crate::CliClient::new($binary),
                }
            }
        }

        impl ::core::default::Default for $name<$crate::JobRunner> {
            fn default() -> Self {
                Self::new()
            }
        }

        impl<R: $crate::Runner> $name<R> {
            /// A client that runs commands through `runner` — pass a fake in tests.
            $vis fn with_runner(runner: R) -> Self {
                $name {
                    core: $crate::CliClient::with_runner($binary, runner),
                }
            }

            /// Kill any command that runs longer than `timeout`.
            $vis fn default_timeout(mut self, timeout: ::core::time::Duration) -> Self {
                self.core = self.core.default_timeout(timeout);
                self
            }
        }
    };
}
