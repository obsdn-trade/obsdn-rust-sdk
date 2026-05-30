# obsdn-sdk developer commands. Run `make` or `make help` for the list.

.DEFAULT_GOAL := help
.PHONY: help fmt fix style lint test doc deny e2e check

## help: show this list of commands
help:
	@echo "obsdn-sdk - make targets:"
	@grep -E '^## ' $(MAKEFILE_LIST) | \
		awk -F': ' '{ sub(/^## /, "", $$1); printf "  \033[36m%-8s\033[0m %s\n", $$1, $$2 }'

## fmt: format the code in place (rustfmt)
fmt:
	cargo fmt

## fix: apply clippy autofixes, then format
fix:
	cargo clippy --all-targets --all-features --fix --allow-dirty --allow-staged
	cargo fmt

## style: fail if anything is unformatted (CI gate)
style:
	cargo fmt --check

## lint: clippy across all targets/features, warnings denied (CI gate)
lint:
	cargo clippy --all-targets --all-features -- -D warnings

## test: offline tests + doctests (live staging tests self-skip)
test:
	cargo test --all-targets
	cargo test --doc

## doc: build docs with warnings/broken links denied (CI gate)
doc:
	RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features

## deny: supply-chain gate - advisories, licenses, bans (needs cargo-deny)
deny:
	cargo deny check

## e2e: run the LIVE staging end-to-end suite (needs network access)
e2e:
	OBSDN_STAGING=1 cargo test --test e2e_staging -- --test-threads=1 --nocapture

## check: full pre-push gate - style + lint + test + doc (mirrors CI)
check: style lint test doc
