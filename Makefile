.PHONY: help build test run docker clean

# Default target
help:
	@echo "Legacy Titan Bridge - Available Commands"
	@echo "========================================="
	@echo ""
	@echo "Development:"
	@echo "  make build      - Build the Rust sidecar"
	@echo "  make test       - Run all tests"
	@echo "  make run        - Run the sidecar locally"
	@echo "  make fmt        - Format Rust code"
	@echo "  make lint       - Run clippy linter"
	@echo ""
	@echo "Docker:"
	@echo "  make docker     - Build Docker image"
	@echo "  make up         - Start with docker-compose"
	@echo "  make down       - Stop docker-compose"
	@echo ""
	@echo "Demo:"
	@echo "  make demo       - Run interactive demo"
	@echo ""
	@echo "Cleanup:"
	@echo "  make clean      - Remove build artifacts"

# ============================================================================
# Development
# ============================================================================

build:
	cd sidecar && cargo build --release

test:
	cd sidecar && cargo test

run:
	cd sidecar && cargo run

fmt:
	cd sidecar && cargo fmt

lint:
	cd sidecar && cargo clippy -- -D warnings

# ============================================================================
# Docker
# ============================================================================

docker:
	docker build -t legacy-titan-bridge:latest .

up:
	docker-compose up --build

down:
	docker-compose down

# ============================================================================
# Demo
# ============================================================================

demo: build
	@echo ""
	@echo "Legacy Titan Bridge demo"
	@echo "====================================="
	@echo ""
	@echo "Starting sidecar in background..."
	@cd sidecar && cargo run &
	@sleep 3
	@echo ""
	@echo "Health check:"
	@curl -s http://localhost:3000/health | jq .
	@echo ""
	@echo "Test transfer, LOW risk:"
	@curl -s -X POST http://localhost:3000/transfer \
		-H "Content-Type: application/json" \
		-d '{"transaction_id":"TX-001","amount":500.00,"from_account":"ACC-A","to_account":"ACC-B"}' | jq .
	@echo ""
	@echo "Test transfer, HIGH risk (over $$10,000):"
	@curl -s -X POST http://localhost:3000/transfer \
		-H "Content-Type: application/json" \
		-d '{"transaction_id":"TX-002","amount":15000.00,"from_account":"ACC-A","to_account":"ACC-C"}' | jq .
	@echo ""
	@echo "Tree status:"
	@curl -s http://localhost:3000/tree-status | jq .
	@echo ""
	@echo "Demo complete! Press Ctrl+C to stop the server."

# ============================================================================
# Cleanup
# ============================================================================

clean:
	cd sidecar && cargo clean
	rm -rf cobol/*.so cobol/*.o
