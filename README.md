# docker-image-tags

This iterates over the tags in a Docker repository, printing each MAJOR.MINOR
branch and the most recent version on that branch in JSON format.

## Building

This is built in Rust, and should have no other prerequisites. I've only tested
it with Rust 1.65.0, but relatively recent versions will probably also work.

## Usage

```bash
cargo run -- -n org -r repo
```

Note that official images live in the `library` namespace; eg:

```bash
cargo run -- -n library -r alpine
```
