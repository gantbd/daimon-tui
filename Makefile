.PHONY: check

check:
	./scripts/check-public-source.sh
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets --locked --offline -- -D warnings
	cargo test --workspace --all-targets --locked --offline
	cargo test --workspace --doc --locked --offline
	RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked --offline
	./tests/live/real_daemon.sh
	bash tests/live/agent_prompt.sh
	bash tests/live/agent_advance.sh
	bash tests/live/agent_permission.sh
	bash tests/live/agent_cancel.sh
	bash tests/live/agent_close.sh
	bash tests/live/agent_reattach.sh
	git diff --check
