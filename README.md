# Flymark

Super-duper-fast CLI imark client, intended for marking exams.


# Installation

Make sure you have a working Rust toolchain installed (stable, latest recommended).

You can install a Rust toolchain with [Rustup](https://rustup.rs/).

Once you have a toolchain installed, simply run: `cargo install flymark`.

# Usage

`flymark <scheme_file> <course> <session>`

* scheme_file is the path to a file that holds the marking
scheme. A sample scheme that displays all the capabilities
is available in `simple_scheme.txt`.

* course is the course to mark, in the format: `cs1521`.

* session is the session of the course to mark, in the format: `22T1`.

There are additional flags to override some of the settings,
including allowing you to use a custom imark cgi endpoint.
Read more with `flymark --help`

## License

Licensed under either of

 * Apache License, Version 2.0
   ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license
   ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
