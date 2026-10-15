import errno
import io
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

import auval


class Clock:
    def __init__(self):
        self.now = 0.0

    def sleep(self, seconds):
        self.now += seconds


def exclusive_rename(source, destination):
    if os.path.lexists(destination):
        raise FileExistsError(str(destination))
    os.rename(source, destination)


class TemporaryComponentTests(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        self.addCleanup(self.folder.cleanup)
        self.root = Path(self.folder.name)
        self.source = self.root / "source.component"
        (self.source / "Contents").mkdir(parents=True)
        (self.source / "Contents/Info.plist").write_bytes(b"complete signed source")
        self.destination = self.root / "Components/Lullaby.component"
        if sys.platform != "darwin":
            native_utime = os.utime

            def emulate_darwin_notification(path, *args, **kwargs):
                # Production is Darwin-only. Windows Python 3.12 cannot perform
                # the nofollow notification syscall, so emulate that one call
                # against this test's known ordinary directory. The test below
                # still observes/asserts the production nofollow flag. Darwin
                # tests retain the actual syscall and never use this adapter.
                if Path(path) == self.destination and kwargs.get("follow_symlinks") is False:
                    self.assertFalse(self.destination.is_symlink())
                    self.assertTrue(self.destination.is_dir())
                    kwargs = dict(kwargs)
                    del kwargs["follow_symlinks"]
                return native_utime(path, *args, **kwargs)

            notification = patch("auval.os.utime", side_effect=emulate_darwin_notification)
            notification.start()
            self.addCleanup(notification.stop)

    def test_complete_verified_bundle_is_published_before_directory_notification(self):
        events = []
        original_utime = os.utime

        def verify(args, **kwargs):
            staged = Path(args[-1])
            self.assertEqual(args[:-1], ["codesign", "--verify", "--deep", "--strict"])
            self.assertEqual(staged.parent, self.destination.parent)
            self.assertTrue(staged.name.startswith("."))
            self.assertFalse(staged.name.endswith(".component"))
            self.assertEqual((staged / "Contents/Info.plist").read_bytes(), b"complete signed source")
            self.assertFalse(os.path.lexists(self.destination))
            events.append("verified")

        def publish(source, destination):
            self.assertEqual(events, ["verified"])
            exclusive_rename(source, destination)
            events.append("published")

        def notify(path, *args, **kwargs):
            if Path(path) == self.destination:
                self.assertIs(kwargs.get("follow_symlinks"), False)
                self.assertEqual(events[-1], "published")
                events.append("bundle notification")
            elif Path(path) == self.destination.parent:
                events.append("directory notification")
            return original_utime(path, *args, **kwargs)

        with patch("auval.subprocess.run", side_effect=verify), \
                patch("auval.rename_exclusive", side_effect=publish), \
                patch("auval.os.utime", side_effect=notify):
            with auval.temporary_component(self.source, self.destination):
                self.assertEqual(events, ["verified", "published", "bundle notification", "directory notification"])
                self.assertEqual((self.destination / "Contents/Info.plist").read_bytes(), b"complete signed source")
            self.assertEqual(events[-1], "directory notification")
        self.assertFalse(self.destination.exists())
        self.assertEqual(list(self.destination.parent.iterdir()), [])

    def test_existing_file_and_directory_are_preserved(self):
        for directory in (False, True):
            with self.subTest(directory=directory):
                self.destination.parent.mkdir(exist_ok=True)
                if directory:
                    self.destination.mkdir()
                    marker = self.destination / "user.txt"
                else:
                    marker = self.destination
                marker.write_bytes(b"previous user installation")
                with patch("auval.subprocess.run") as run, self.assertRaises(FileExistsError):
                    with auval.temporary_component(self.source, self.destination):
                        self.fail("existing path must not be replaced")
                self.assertEqual(marker.read_bytes(), b"previous user installation")
                run.assert_not_called()
                if directory:
                    shutil.rmtree(self.destination)
                else:
                    self.destination.unlink()

    def test_existing_dangling_symlink_is_preserved(self):
        self.destination.parent.mkdir()
        try:
            self.destination.symlink_to(self.root / "absent")
        except OSError:
            self.skipTest("Symlink creation unavailable on this host")
        with self.assertRaises(FileExistsError):
            with auval.temporary_component(self.source, self.destination):
                self.fail("dangling symlink must not be replaced")
        self.assertTrue(self.destination.is_symlink())
        self.assertEqual(os.readlink(self.destination), str(self.root / "absent"))

    def test_publication_race_preserves_new_external_installation(self):
        def raced_publish(source, destination):
            destination.mkdir()
            (destination / "user.txt").write_bytes(b"concurrent user installation")
            exclusive_rename(source, destination)

        with patch("auval.subprocess.run"), \
                patch("auval.rename_exclusive", side_effect=raced_publish), self.assertRaises(FileExistsError):
            with auval.temporary_component(self.source, self.destination):
                self.fail("concurrent installation must not be replaced")
        self.assertEqual((self.destination / "user.txt").read_bytes(), b"concurrent user installation")
        self.assertEqual(list(self.destination.parent.iterdir()), [self.destination])

    def test_signature_failure_cleans_staging_without_publication(self):
        with patch("auval.subprocess.run", side_effect=subprocess.CalledProcessError(1, "codesign")), \
                patch("auval.rename_exclusive") as publish, self.assertRaises(subprocess.CalledProcessError):
            with auval.temporary_component(self.source, self.destination):
                self.fail("invalid staging signature must not be published")
        publish.assert_not_called()
        self.assertEqual(list(self.destination.parent.iterdir()), [])

    def test_validator_exception_cleans_owned_installation(self):
        with patch("auval.subprocess.run"), patch("auval.rename_exclusive", side_effect=exclusive_rename):
            with self.assertRaisesRegex(RuntimeError, "validator failed"):
                with auval.temporary_component(self.source, self.destination):
                    raise RuntimeError("validator failed")
        self.assertEqual(list(self.destination.parent.iterdir()), [])

    def test_cleanup_does_not_remove_replacement_installation(self):
        with patch("auval.subprocess.run"), patch("auval.rename_exclusive", side_effect=exclusive_rename):
            with auval.temporary_component(self.source, self.destination):
                self.destination.rename(self.root / "moved-owned-copy")
                self.destination.mkdir()
                (self.destination / "user.txt").write_bytes(b"replacement owned by user")
        self.assertEqual((self.destination / "user.txt").read_bytes(), b"replacement owned by user")

    def test_strict_validation_runs_after_discovery_and_preserves_exit_code(self):
        calls = []

        def run(args, **kwargs):
            calls.append(args)
            if args == ["auval", "-l"]:
                self.assertTrue((self.destination / "Contents/Info.plist").is_file())
                return subprocess.CompletedProcess(args, 0, "aufx Lulb Llby - Lullaby Audio: Lullaby\n", "")
            if args[0] == "codesign":
                return subprocess.CompletedProcess(args, 0)
            return subprocess.CompletedProcess(args, 47)

        with patch("auval.subprocess.run", side_effect=run), \
                patch("auval.rename_exclusive", side_effect=exclusive_rename):
            self.assertEqual(auval.validate(self.source, self.destination), 47)
        self.assertEqual(calls[-2:], [["auval", "-l"], ["auval", "-v", "aufx", "Lulb", "Llby", "-strict"]])
        self.assertFalse(self.destination.exists())


class DiscoveryTests(unittest.TestCase):
    def setUp(self):
        self.clock = Clock()
        patch("auval.time.monotonic", side_effect=lambda: self.clock.now).start()
        patch("auval.time.sleep", side_effect=self.clock.sleep).start()
        self.addCleanup(patch.stopall)

    def test_delayed_registration_after_old_twenty_second_limit_succeeds(self):
        def listing(args, **kwargs):
            text = "aufx Lulb Llby - Lullaby Audio: Lullaby\n" if self.clock.now >= 25 else ""
            self.assertLessEqual(kwargs["timeout"], 10)
            return subprocess.CompletedProcess(args, 0, text, "")

        with patch("auval.subprocess.run", side_effect=listing) as run:
            auval.wait_for_discovery()
        self.assertEqual(self.clock.now, 25)
        self.assertTrue(all(item.args[0] == ["auval", "-l"] for item in run.call_args_list))

    def test_only_successful_listing_with_exact_three_identifier_fields_is_accepted(self):
        results = [
            subprocess.CompletedProcess([], 0, "other aufx Lulb Llby\n", ""),
            subprocess.CompletedProcess([], 1, "aufx Lulb Llby - Lullaby\n", "registrar unavailable"),
            subprocess.CompletedProcess([], 0, "aufx Lulb Llby - Lullaby\n", ""),
        ]
        with patch("auval.subprocess.run", side_effect=results) as run:
            auval.wait_for_discovery()
        self.assertEqual(run.call_count, 3)
        self.assertEqual(self.clock.now, 1)

    def test_timeout_is_bounded_and_reports_last_listing_stderr_and_status(self):
        result = subprocess.CompletedProcess([], 7, "aufx Other Vend - Different plugin\n", "registrar busy")
        with patch("auval.subprocess.run", return_value=result), \
                self.assertRaisesRegex(RuntimeError, "within 1.5 seconds") as error:
            auval.wait_for_discovery(1.5)
        self.assertEqual(self.clock.now, 1.5)
        self.assertIn("status: 7", str(error.exception))
        self.assertIn("Different plugin", str(error.exception))
        self.assertIn("registrar busy", str(error.exception))

    def test_hung_listing_is_also_bounded(self):
        def blocked(args, **kwargs):
            self.clock.now += kwargs["timeout"]
            raise subprocess.TimeoutExpired(args, kwargs["timeout"])

        with patch("auval.subprocess.run", side_effect=blocked), \
                self.assertRaisesRegex(RuntimeError, "listing timed out"):
            auval.wait_for_discovery(2)
        self.assertEqual(self.clock.now, 2)

    def test_nonfinite_and_nonpositive_timeouts_are_rejected(self):
        with patch("auval.subprocess.run") as run:
            for timeout in (0, -1, float("nan"), float("inf")):
                with self.subTest(timeout=timeout), self.assertRaises(ValueError):
                    auval.wait_for_discovery(timeout)
            run.assert_not_called()


class ExclusiveRenameTests(unittest.TestCase):
    def test_unsupported_exclusive_rename_fails_without_overwriting_fallback(self):
        operation = Mock(return_value=-1)
        library = Mock(renamex_np=operation)
        with patch("auval.ctypes.CDLL", return_value=library), \
                patch("auval.ctypes.get_errno", return_value=errno.ENOTSUP), \
                patch("auval.os.rename") as unsafe_fallback, self.assertRaises(OSError) as error:
            auval.rename_exclusive(Path("stage"), Path("destination"))
        self.assertEqual(error.exception.errno, errno.ENOTSUP)
        self.assertEqual(operation.call_args.args[-1], 0x4)
        unsafe_fallback.assert_not_called()

    @unittest.skipUnless(sys.platform == "darwin", "Darwin exclusive rename syscall")
    def test_native_exclusive_publication_refuses_file_directory_and_dangling_symlink(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            for kind in ("file", "directory", "symlink"):
                with self.subTest(kind=kind):
                    source = root / "stage"
                    source.mkdir()
                    (source / "payload").write_bytes(b"ours")
                    destination = root / "published.component"
                    if kind == "file":
                        destination.write_bytes(b"existing")
                    elif kind == "directory":
                        destination.mkdir()
                        (destination / "existing").write_bytes(b"existing")
                    else:
                        destination.symlink_to(root / "absent")
                    with self.assertRaises(FileExistsError):
                        auval.rename_exclusive(source, destination)
                    self.assertEqual((source / "payload").read_bytes(), b"ours")
                    if kind == "directory":
                        self.assertEqual((destination / "existing").read_bytes(), b"existing")
                        shutil.rmtree(destination)
                    else:
                        self.assertTrue(os.path.lexists(destination))
                        destination.unlink()
                    auval.rename_exclusive(source, destination)
                    self.assertEqual((destination / "payload").read_bytes(), b"ours")
                    self.assertFalse(source.exists())
                    shutil.rmtree(destination)


class CiRegistrarRefreshTests(unittest.TestCase):
    def test_refresh_is_refused_outside_macos_github_actions_before_installation(self):
        for platform, actions in (("darwin", "false"), ("linux", "true"), ("win32", "true")):
            with self.subTest(platform=platform, actions=actions), \
                    patch("auval.sys.platform", platform), patch.dict(os.environ, {"GITHUB_ACTIONS": actions}), \
                    patch("auval.temporary_component") as install, self.assertRaisesRegex(RuntimeError, "only in a macOS"):
                auval.validate(Path("source"), Path("destination"), ci_refresh_registrar=True)
            install.assert_not_called()

    def test_refresh_command_targets_only_exact_registrar_and_current_uid_account(self):
        lookup = Mock(return_value=SimpleNamespace(pw_name="isolated-runner"))
        with patch("auval.sys.platform", "darwin"), patch.dict(os.environ, {"GITHUB_ACTIONS": "true"}), \
                patch("auval.os.getuid", return_value=501, create=True), \
                patch.dict(sys.modules, {"pwd": SimpleNamespace(getpwuid=lookup)}), \
                patch("auval.subprocess.run", return_value=subprocess.CompletedProcess([], 0, "", "")) as run, \
                patch("sys.stdout", new_callable=io.StringIO):
            auval.refresh_ci_registrar()
        lookup.assert_called_once_with(501)
        run.assert_called_once_with(["killall", "-u", "isolated-runner", "AudioComponentRegistrar"],
                                    capture_output=True, text=True, check=False, timeout=10)

    def test_no_matching_registrar_still_allows_bounded_rediscovery(self):
        with patch("auval.require_ci_refresh"), \
                patch("auval.os.getuid", return_value=501, create=True), \
                patch.dict(sys.modules, {"pwd": SimpleNamespace(getpwuid=lambda _: SimpleNamespace(pw_name="runner"))}), \
                patch("auval.subprocess.run", return_value=subprocess.CompletedProcess([], 1, "", "no matching process")), \
                patch("sys.stdout", new_callable=io.StringIO):
            auval.refresh_ci_registrar()

    def test_initial_discovery_success_never_refreshes_registrar(self):
        with patch("auval.require_ci_refresh"), patch("auval.temporary_component"), \
                patch("auval.wait_for_discovery") as wait, patch("auval.refresh_ci_registrar") as refresh, \
                patch("auval.subprocess.run", return_value=subprocess.CompletedProcess([], 0)):
            self.assertEqual(auval.validate(Path("source"), Path("destination"), 12, True), 0)
        wait.assert_called_once_with(12)
        refresh.assert_not_called()

    def test_only_one_refresh_and_one_bounded_retry_follow_initial_timeout(self):
        with patch("auval.require_ci_refresh"), patch("auval.temporary_component"), \
                patch("auval.wait_for_discovery", side_effect=[auval.DiscoveryTimeout("first timeout"), None]) as wait, \
                patch("auval.refresh_ci_registrar") as refresh, \
                patch("auval.subprocess.run", return_value=subprocess.CompletedProcess([], 0)) as run, \
                patch("sys.stdout", new_callable=io.StringIO):
            self.assertEqual(auval.validate(Path("source"), Path("destination"), 12, True), 0)
        self.assertEqual(wait.call_count, 2)
        self.assertTrue(all(call.args == (12,) for call in wait.call_args_list))
        refresh.assert_called_once_with()
        run.assert_called_once_with(["auval", "-v", "aufx", "Lulb", "Llby", "-strict"], check=False)

    def test_final_timeout_fails_and_exits_installation_context_without_validation(self):
        with patch("auval.require_ci_refresh"), patch("auval.temporary_component") as install, \
                patch("auval.wait_for_discovery", side_effect=[auval.DiscoveryTimeout("first"), auval.DiscoveryTimeout("final")]) as wait, \
                patch("auval.refresh_ci_registrar") as refresh, patch("auval.subprocess.run") as run, \
                patch("sys.stdout", new_callable=io.StringIO), self.assertRaisesRegex(auval.DiscoveryTimeout, "final"):
            auval.validate(Path("source"), Path("destination"), 12, True)
        self.assertEqual(wait.call_count, 2)
        refresh.assert_called_once_with()
        install.return_value.__exit__.assert_called_once()
        run.assert_not_called()

    def test_default_discovery_timeout_does_not_restart_service(self):
        with patch("auval.temporary_component"), \
                patch("auval.wait_for_discovery", side_effect=auval.DiscoveryTimeout("timeout")) as wait, \
                patch("auval.refresh_ci_registrar") as refresh, self.assertRaises(auval.DiscoveryTimeout):
            auval.validate(Path("source"), Path("destination"))
        wait.assert_called_once_with(60.0)
        refresh.assert_not_called()

    def test_actual_strict_validation_failure_is_not_retried_or_refreshed(self):
        with patch("auval.require_ci_refresh"), patch("auval.temporary_component"), \
                patch("auval.wait_for_discovery"), patch("auval.refresh_ci_registrar") as refresh, \
                patch("auval.subprocess.run", return_value=subprocess.CompletedProcess([], 9)) as run:
            self.assertEqual(auval.validate(Path("source"), Path("destination"), 12, True), 9)
        run.assert_called_once()
        refresh.assert_not_called()

    def test_other_setup_errors_are_not_retried_or_refreshed(self):
        with patch("auval.require_ci_refresh"), patch("auval.temporary_component"), \
                patch("auval.wait_for_discovery", side_effect=OSError("missing auval")) as wait, \
                patch("auval.refresh_ci_registrar") as refresh, self.assertRaisesRegex(OSError, "missing auval"):
            auval.validate(Path("source"), Path("destination"), 12, True)
        wait.assert_called_once_with(12)
        refresh.assert_not_called()


if __name__ == "__main__":
    unittest.main()
