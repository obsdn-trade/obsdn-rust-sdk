.PHONY: fmt style lint test doc check

# Apply rustfmt in place.
fmt:
	cargo fmt

# Style gate — fail if anything is unformatted (mirrors CI `cargo fmt --check`).
style:
	cargo fmt --check

# Clippy with warnings denied (mirrors CI).
lint:
	cargo clippy --all-targets -- -D warnings

# Offline test suite (mirrors CI). Live e2e self-skip without OBSDN_STAGING.
test:
	cargo test --all-targets

# Doc build with broken-link/warning denial (mirrors CI).
doc:
	RUSTDOCFLAGS="-D warnings" cargo doc --no-deps

# Full pre-push gate: everything CI runs.
check: style lint test doc
