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
from git_collaboration.task_numbers import task_numbers


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--native-only", action="store_true", help="Prepare fresh native clones without rerunning the Python scenarios")
    parser.add_argument("--numbering-only", action="store_true", help="Run the focused task-number MCP regression")
    args = parser.parse_args()
    suite = Suite(args.binary)
    try:
        if args.numbering_only:
            task_numbers(suite)
        elif not args.native_only:
            additions(suite)
            task_numbers(suite)
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
