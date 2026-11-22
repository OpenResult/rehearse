import json
from pathlib import Path
import tempfile
import unittest

from workflow_e2e import MARKER, prepare_root, validate_reports


class WorkflowE2ETests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.base = Path(self.temporary.name).resolve()

    def tearDown(self):
        self.temporary.cleanup()

    def test_output_roots_preserve_previous_runs_and_refuse_unowned_data(self):
        unowned = self.base / "unowned"
        unowned.mkdir()
        sentinel = unowned / "keep.txt"
        sentinel.write_text("existing user data")
        with self.assertRaisesRegex(ValueError, "nonempty"):
            prepare_root(unowned)
        self.assertEqual(sentinel.read_text(), "existing user data")

        owned = prepare_root(self.base / "owned")
        previous = owned / "previous-run"
        previous.mkdir()
        (previous / "report.json").write_text("retained")
        self.assertEqual(prepare_root(owned), owned)
        self.assertEqual((previous / "report.json").read_text(), "retained")
        self.assertTrue((owned / MARKER).is_file())

    def test_output_roots_refuse_symlink_parents_markers_and_parent_traversal(self):
        target = self.base / "target"
        target.mkdir()
        link = self.base / "link"
        link.symlink_to(target, target_is_directory=True)
        for path in (link, link / "nested", self.base / "missing" / ".." / "other"):
            with self.assertRaises(ValueError):
                prepare_root(path)
        marker_link = self.base / "marker-link"
        marker_link.mkdir()
        (marker_link / MARKER).symlink_to(target / "anything")
        with self.assertRaisesRegex(ValueError, "marker"):
            prepare_root(marker_link)
        self.assertEqual(list(target.iterdir()), [])

    def test_report_validation_rejects_lost_events_and_fabricated_execution_times(self):
        directory = self.base / "report"
        directory.mkdir()
        graph = {"schema_version": 1, "plan_name": "release", "plan_revision": None,
                 "nodes": [{"id": 0, "name": "verify"}, {"id": 1, "name": "publish"}]}
        report = {"schema_version": 1, "plan_name": "release", "plan_revision": None,
                  "mode": "execute", "outcome": "failed", "nodes": [
                      {"id": 0, "name": "verify", "status": "failed", "duration_ms": 1},
                      {"id": 1, "name": "publish", "status": "not_started", "duration_ms": None}]}
        events = [{"schema_version": 1, "sequence": 1, "kind": "plan_started",
                   "plan_name": "release", "mode": "execute", "node_id": None},
                  {"schema_version": 1, "sequence": 2, "kind": "plan_finished",
                   "plan_name": "release", "mode": "execute", "node_id": None}]
        (directory / "graph.json").write_text(json.dumps(graph))
        (directory / "report.json").write_text(json.dumps(report))
        (directory / "summary.md").write_text("## Retained outcome\n")

        def write_events():
            (directory / "events.jsonl").write_text("\n".join(json.dumps(event) for event in events) + "\n")

        write_events()
        self.assertEqual(validate_reports(directory, graph, "execute", "failed")["publish"], "not_started")
        events[-1]["sequence"] = 3
        write_events()
        with self.assertRaisesRegex(AssertionError, "sequence"):
            validate_reports(directory, graph, "execute", "failed")
        events[-1]["sequence"] = 2
        write_events()
        report["nodes"][1]["duration_ms"] = 0
        (directory / "report.json").write_text(json.dumps(report))
        with self.assertRaisesRegex(AssertionError, "unstarted"):
            validate_reports(directory, graph, "execute", "failed")


if __name__ == "__main__":
    unittest.main()
