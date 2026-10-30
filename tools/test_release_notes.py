"""Check generated public setup instructions for both release paths."""
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TAG = "v0.0.1-main.999"


def run(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True)


class ReleaseNotesTest(unittest.TestCase):
    def check_notes(self, notes):
        self.assertNotIn("{{TAG}}", notes)
        section = notes.split("## Run the server with Docker\n", 1)[1].split("\n## ", 1)[0]
        self.assertIn(f"ghcr.io/xaigroking/onepaste-server:{TAG}", section)
        self.assertNotIn("onepaste-server:edge", section)
        self.assertIn("https://YOUR_SERVER_PUBLIC_IP:8443", section)
        self.assertIn("-v onepaste-data:/data", section)
        # These ordered actions must survive generation, after the server command.
        steps = ["docker run", "docker exec onepaste /onepaste-server connect-code",
                 "**First desktop.**", "**Second desktop.**", "**Check both directions.**"]
        for step in steps:
            self.assertIn(step, section)
        positions = [section.index(step) for step in steps]
        self.assertEqual(positions, sorted(positions))
        first = section[positions[2]:positions[3]]
        for instruction in ["portable zip", "First launch", "Connect to my own server",
                            "Next", "passphrase", "recovery kit", "I have saved it somewhere offline", "Done", "Join"]:
            self.assertIn(instruction, first)
        second = section[positions[3]:positions[4]]
        for instruction in ["Devices", "Add a device", "Make a connection code", "same passphrase", "same server"]:
            self.assertIn(instruction, second)
        check = section[positions[4]:]
        for instruction in ["computer A", "B's history", "different text on B", "A's history", "can be copied", "healthy container"]:
            self.assertIn(instruction, check)
        self.assertNotRegex(section, r"onepaste join|npm |cargo |git clone|http://")

    def test_automatic_release(self):
        commit = run("git", "rev-parse", "HEAD").strip()
        self.check_notes(run("bash", "scripts/make-release-notes.sh", TAG, commit, commit))

    def test_named_release(self):
        # Execute the actual shared-template assembly from the named release job.
        workflow = (ROOT / ".github/workflows/release.yml").read_text()
        command = next(line.strip() for line in workflow.splitlines()
                       if 'sed "s/{{TAG}}/$GITHUB_REF_NAME/g"' in line)
        with tempfile.TemporaryDirectory() as directory:
            # The assembly uses only public templates and the generated download list.
            work = Path(directory)
            (work / "packaging").symlink_to(ROOT / "packaging", target_is_directory=True)
            (work / "downloads.md").write_text("## Downloads\nTest downloads\n")
            subprocess.run(["bash", "-eu", "-c", command], cwd=work,
                           env={**os.environ, "GITHUB_REF_NAME": TAG}, check=True)
            self.check_notes((work / "notes.md").read_text())


if __name__ == "__main__":
    unittest.main()
