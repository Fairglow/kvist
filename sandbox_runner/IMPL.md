<!-- kvist-implementation-record-version: 1 -->
# Component Implementation Record

## Observed scaffold

The Rust package builds a library and an executable named
`kvist-sandbox-runner`. The library exposes a constant status string identifying
the package as a scaffold.

## Observed command behavior

The executable writes the scaffold status to standard error and exits with
status 2 for every invocation.

## Unimplemented behavior

No sandbox probe, request parser, Bubblewrap invocation, namespace setup,
grant validation, resource enforcement, task execution, or evidence protocol
is implemented.
