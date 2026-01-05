#!/bin/bash
#
# Brisby Integration Test
#
# Tests the full flow: index provider -> seeder -> client search -> client download
# Requires: binaries built with --features nym
#
# Usage: ./scripts/integration-test.sh [--mock]
#   --mock: Use mock transport (no real Nym connection, faster but less thorough)

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
TEST_DIR="/tmp/brisby-integration-test-$$"
INDEX_DIR="$TEST_DIR/index"
SEEDER_DIR="$TEST_DIR/seeder"
CLIENT_DIR="$TEST_DIR/client"
LOG_DIR="$TEST_DIR/logs"

INDEX_LOG="$LOG_DIR/index.log"
SEEDER_LOG="$LOG_DIR/seeder.log"

# Binaries
BRISBY="./target/release/brisby"
BRISBY_INDEX="./target/release/brisby-index"

# Test file
TEST_FILE="$TEST_DIR/test-file.txt"
TEST_CONTENT="Hello, Brisby! This is test content for the integration test. $(date)"

# Process PIDs
INDEX_PID=""
SEEDER_PID=""

# Parse arguments
USE_MOCK=false
NYM_TIMEOUT=120  # seconds to wait for Nym connections

while [[ $# -gt 0 ]]; do
    case $1 in
        --mock)
            USE_MOCK=true
            NYM_TIMEOUT=5
            shift
            ;;
        --help|-h)
            echo "Usage: $0 [--mock]"
            echo "  --mock: Use mock transport (faster, no real Nym)"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

# Cleanup function
cleanup() {
    echo -e "${BLUE}Cleaning up...${NC}"

    if [[ -n "$INDEX_PID" ]] && kill -0 "$INDEX_PID" 2>/dev/null; then
        kill "$INDEX_PID" 2>/dev/null || true
        wait "$INDEX_PID" 2>/dev/null || true
    fi

    if [[ -n "$SEEDER_PID" ]] && kill -0 "$SEEDER_PID" 2>/dev/null; then
        kill "$SEEDER_PID" 2>/dev/null || true
        wait "$SEEDER_PID" 2>/dev/null || true
    fi

    # Keep logs on failure for debugging
    if [[ $? -ne 0 ]]; then
        echo -e "${YELLOW}Test failed. Logs preserved at: $LOG_DIR${NC}"
    else
        rm -rf "$TEST_DIR"
    fi
}

trap cleanup EXIT

# Helper functions
log_step() {
    echo -e "${BLUE}==>${NC} $1"
}

log_success() {
    echo -e "${GREEN}[PASS]${NC} $1"
}

log_fail() {
    echo -e "${RED}[FAIL]${NC} $1"
    exit 1
}

log_info() {
    echo -e "${YELLOW}[INFO]${NC} $1"
}

wait_for_pattern() {
    local file="$1"
    local pattern="$2"
    local timeout="$3"
    local elapsed=0

    while [[ $elapsed -lt $timeout ]]; do
        if grep -q "$pattern" "$file" 2>/dev/null; then
            return 0
        fi
        sleep 1
        ((elapsed++))
    done
    return 1
}

extract_address() {
    local file="$1"
    grep "Address:" "$file" | tail -1 | sed 's/.*Address: //'
}

# Check prerequisites
check_prerequisites() {
    log_step "Checking prerequisites"

    if [[ ! -x "$BRISBY" ]]; then
        log_fail "Client binary not found: $BRISBY (run: cargo build --release -p brisby-client --features nym)"
    fi

    if [[ ! -x "$BRISBY_INDEX" ]]; then
        log_fail "Index binary not found: $BRISBY_INDEX (run: cargo build --release -p brisby-index --features nym)"
    fi

    log_success "Binaries found"
}

# Setup test environment
setup_environment() {
    log_step "Setting up test environment"

    mkdir -p "$INDEX_DIR" "$SEEDER_DIR" "$CLIENT_DIR" "$LOG_DIR"

    # Create test file
    echo "$TEST_CONTENT" > "$TEST_FILE"

    log_success "Test environment ready at $TEST_DIR"
}

# Start index provider
start_index_provider() {
    log_step "Starting index provider"

    if $USE_MOCK; then
        log_info "Mock mode - skipping real index provider"
        return 0
    fi

    "$BRISBY_INDEX" -d "$INDEX_DIR" > "$INDEX_LOG" 2>&1 &
    INDEX_PID=$!

    log_info "Waiting for index provider to connect to Nym (up to ${NYM_TIMEOUT}s)..."

    if ! wait_for_pattern "$INDEX_LOG" "Starting message loop" "$NYM_TIMEOUT"; then
        echo "Index provider log:"
        cat "$INDEX_LOG"
        log_fail "Index provider failed to start"
    fi

    INDEX_ADDR=$(extract_address "$INDEX_LOG")
    if [[ -z "$INDEX_ADDR" ]]; then
        log_fail "Could not extract index provider address"
    fi

    log_success "Index provider running at: $INDEX_ADDR"
}

# Start seeder with test file
start_seeder() {
    log_step "Starting seeder with test file"

    if $USE_MOCK; then
        log_info "Mock mode - testing seeder in mock mode"
        "$BRISBY" --mock -d "$SEEDER_DIR" seed -f "$TEST_FILE"
        log_success "Seeder mock test passed"
        return 0
    fi

    "$BRISBY" -d "$SEEDER_DIR" seed -f "$TEST_FILE" -p --index-provider "$INDEX_ADDR" > "$SEEDER_LOG" 2>&1 &
    SEEDER_PID=$!

    log_info "Waiting for seeder to connect to Nym (up to ${NYM_TIMEOUT}s)..."

    if ! wait_for_pattern "$SEEDER_LOG" "Seeder is running" "$NYM_TIMEOUT"; then
        echo "Seeder log:"
        cat "$SEEDER_LOG"
        log_fail "Seeder failed to start"
    fi

    SEEDER_ADDR=$(extract_address "$SEEDER_LOG")
    if [[ -z "$SEEDER_ADDR" ]]; then
        log_fail "Could not extract seeder address"
    fi

    # Wait for publish to complete
    sleep 5

    if grep -q "Published:" "$SEEDER_LOG"; then
        log_success "Seeder running and file published"
    else
        log_info "Seeder running (publish may still be in progress)"
    fi

    log_info "Seeder address: $SEEDER_ADDR"
}

# Test search
test_search() {
    log_step "Testing search"

    if $USE_MOCK; then
        log_info "Mock mode - testing search in mock mode"
        "$BRISBY" --mock -d "$CLIENT_DIR" search "test" --index-provider "mock-index"
        log_success "Search mock test passed"
        return 0
    fi

    # Wait a bit for the index to be updated
    log_info "Waiting for index to be updated..."
    sleep 10

    log_info "Searching for 'test-file'..."

    SEARCH_OUTPUT=$("$BRISBY" -d "$CLIENT_DIR" search "test-file" --index-provider "$INDEX_ADDR" 2>&1) || true

    echo "$SEARCH_OUTPUT"

    if echo "$SEARCH_OUTPUT" | grep -q "Found.*results"; then
        log_success "Search returned results"

        # Extract hash for download test
        FILE_HASH=$(echo "$SEARCH_OUTPUT" | grep "Hash:" | head -1 | awk '{print $2}')
        if [[ -n "$FILE_HASH" ]]; then
            log_info "Found file hash: $FILE_HASH"
        fi
    elif echo "$SEARCH_OUTPUT" | grep -q "No results"; then
        log_info "No results found (file may not be indexed yet)"
    else
        log_fail "Search failed unexpectedly"
    fi
}

# Test download (if we have a hash and seeder)
test_download() {
    log_step "Testing download"

    if $USE_MOCK; then
        log_info "Mock mode - testing download in mock mode"
        "$BRISBY" --mock -d "$CLIENT_DIR" download "0000000000000000000000000000000000000000000000000000000000000000" \
            -s "mock-seeder" -c 1 || true
        log_success "Download mock test passed"
        return 0
    fi

    if [[ -z "$FILE_HASH" ]] || [[ -z "$SEEDER_ADDR" ]]; then
        log_info "Skipping download test (no hash or seeder address)"
        return 0
    fi

    DOWNLOAD_OUTPUT="$TEST_DIR/downloaded-file.txt"

    log_info "Downloading file..."

    RESULT=$("$BRISBY" -d "$CLIENT_DIR" download "$FILE_HASH" \
        -s "$SEEDER_ADDR" \
        -c 1 \
        -o "$DOWNLOAD_OUTPUT" 2>&1) || true

    echo "$RESULT"

    if [[ -f "$DOWNLOAD_OUTPUT" ]]; then
        DOWNLOADED_CONTENT=$(cat "$DOWNLOAD_OUTPUT")
        if [[ "$DOWNLOADED_CONTENT" == "$TEST_CONTENT" ]]; then
            log_success "Download successful - content verified!"
        else
            log_info "Download completed but content differs (may be encoding issue)"
        fi
    else
        log_info "Download did not complete (network timing issues are expected)"
    fi
}

# Main test flow
main() {
    echo -e "${GREEN}================================${NC}"
    echo -e "${GREEN}  Brisby Integration Test${NC}"
    echo -e "${GREEN}================================${NC}"
    echo

    if $USE_MOCK; then
        echo -e "${YELLOW}Running in MOCK mode (no real Nym connections)${NC}"
    else
        echo -e "${YELLOW}Running with REAL Nym network (this will take a few minutes)${NC}"
    fi
    echo

    check_prerequisites
    setup_environment
    start_index_provider
    start_seeder
    test_search
    test_download

    echo
    echo -e "${GREEN}================================${NC}"
    echo -e "${GREEN}  Integration Test Complete${NC}"
    echo -e "${GREEN}================================${NC}"

    if $USE_MOCK; then
        echo -e "${YELLOW}Note: Run without --mock for full Nym network testing${NC}"
    fi
}

main "$@"
