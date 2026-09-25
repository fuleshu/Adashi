"""Task 19 acceptance: real Git clones, actual MCP processes and retained evidence."""
import argparse
import sys
import traceback

sys.dont_write_bytecode = True
from git_collaboration.common import Suite, write_json
from git_collaboration.merges import additions, edits, local_processes_and_reads, legacy_request_cleanup
from git_collaboration.conflicts import textual, semantic, ordering
from git_collaboration.recovery import interrupted, prepare_native
from git_collaboration.markdown import markdown


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--native-only", action="store_true", help="Prepare fresh native clones without rerunning the Python scenarios")
    args = parser.parse_args()
    suite = Suite(args.binary)
    try:
        if not args.native_only:
            additions(suite)
            edits(suite)
            markdown(suite)
            local_processes_and_reads(suite)
            legacy_request_cleanup(suite)
            textual(suite)
            semantic(suite)
            ordering(suite)
            interrupted(suite)
        prepare_native(suite)
        suite.evidence["preparedForNative" if args.native_only else "success"] = True
    except Exception:
        suite.evidence["error"] = traceback.format_exc()
        raise
    finally:
        suite.flush()
        print("Evidence:", suite.root, flush=True)


if __name__ == "__main__":
    main()
