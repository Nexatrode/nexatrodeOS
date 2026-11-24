"""Photo-enabled builds must not mix old UI, reader, or vault binaries."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("accelerated", ROOT / "experiments/hvf-smp/accelerated.py")
accelerated = importlib.util.module_from_spec(spec)
spec.loader.exec_module(accelerated)


class PhotoBuildTest(unittest.TestCase):
    def test_missing_reader_or_old_ui_requires_source_rebuild(self):
        with tempfile.TemporaryDirectory() as tmp:
            pds = Path(tmp)
            ui = pds / "slint_notes_utm.elf"
            worker = pds / "slint_photo_worker.elf"
            reader = pds / "slint_photo_reader.elf"
            ui.write_bytes(b"legacy ui")
            self.assertTrue(accelerated.ui_source_required(pds))
            worker.write_bytes(b"worker")
            self.assertTrue(accelerated.ui_source_required(pds))
            ui.write_bytes(b"elf with photo_mailbox_region and photo_read_mailbox")
            self.assertTrue(accelerated.ui_source_required(pds))
            reader.write_bytes(b"reader")
            transcript = pds / "slint_transcript_worker.elf"
            messages = pds / "rust_messages_agent_app.elf"
            entropy = pds / "rust_entropy_rng.elf"
            entropy.write_bytes(b"ENTROPY|DEVICE_LOCK_V1")
            transcript.write_bytes(b"transcript_mailbox")
            messages.write_bytes(b"messages_worker_transfer LIVE_RECEIVE_WORKER_ABI_V1 NAVIGATION_WORKER_ABI_V1 SNAPSHOT_WORKER_ABI_V1")
            ui.write_bytes(b"photo_mailbox_region photo_read_mailbox transcript_mailbox")
            self.assertTrue(accelerated.ui_source_required(pds))
            ui.write_bytes(b"photo_mailbox_region photo_read_mailbox transcript_mailbox live_receive_mailbox")
            self.assertTrue(accelerated.ui_source_required(pds))
            transcript.write_bytes(b"transcript_mailbox live_receive_mailbox")
            self.assertTrue(accelerated.ui_source_required(pds))
            ui.write_bytes(b"photo_mailbox_region photo_read_mailbox transcript_mailbox live_receive_mailbox navigation_mailbox")
            self.assertTrue(accelerated.ui_source_required(pds))
            transcript.write_bytes(b"transcript_mailbox live_receive_mailbox navigation_mailbox")
            self.assertTrue(accelerated.ui_source_required(pds))
            ui.write_bytes(b"photo_mailbox_region photo_read_mailbox transcript_mailbox live_receive_mailbox navigation_mailbox snapshot_mailbox")
            self.assertTrue(accelerated.ui_source_required(pds))
            transcript.write_bytes(b"transcript_mailbox live_receive_mailbox navigation_mailbox snapshot_mailbox")
            self.assertTrue(accelerated.ui_source_required(pds))
            ui.write_bytes(b"photo_mailbox_region photo_read_mailbox transcript_mailbox live_receive_mailbox navigation_mailbox snapshot_mailbox device_lock_transfer")
            self.assertTrue(accelerated.ui_source_required(pds))
            ui.write_bytes(ui.read_bytes() + b" slint_clock_view")
            self.assertTrue(accelerated.ui_source_required(pds))
            ui.write_bytes(ui.read_bytes() + b" SLINT|FRAME_COPY_RAM_V1")
            self.assertFalse(accelerated.ui_source_required(pds))
            entropy.write_bytes(b"old entropy service")
            self.assertTrue(accelerated.ui_source_required(pds))
            entropy.write_bytes(b"ENTROPY|DEVICE_LOCK_V1")
            reader.unlink()
            self.assertTrue(accelerated.ui_source_required(pds))

    def test_reader_uses_a_separate_vault_transfer_page(self):
        system = ET.parse(ROOT / "experiments/hvf-smp/accelerated.system")
        reader = system.find("protection_domain[@name='photo_reader']")
        vault = system.find("protection_domain[@name='vault']")
        ui = system.find("protection_domain[@name='slint_ui']")
        self.assertIsNotNone(reader)
        reader_maps = {item.get("mr") for item in reader.findall("map")}
        vault_maps = {item.get("mr") for item in vault.findall("map")}
        ui_maps = {item.get("mr") for item in ui.findall("map")}
        self.assertIn("photo_read_transfer", reader_maps & vault_maps)
        self.assertNotIn("messages_transfer", reader_maps)
        self.assertNotIn("photo_read_transfer", ui_maps)
        self.assertIn("photo_read_mailbox", reader_maps & ui_maps)
        channels = system.findall("channel")
        self.assertTrue(any(any(end.get("pd") == "photo_reader" and end.get("id") == "1" and end.get("pp") == "true" for end in channel)
                            and any(end.get("pd") == "vault" and end.get("id") == "9" for end in channel)
                            for channel in channels))

    def test_old_vault_requires_source_rebuild(self):
        with tempfile.TemporaryDirectory() as tmp:
            pds = Path(tmp)
            vault = pds / "rust_vault_disk.elf"
            vault.write_bytes(b"legacy vault")
            self.assertTrue(accelerated.vault_source_required(pds))
            vault.write_bytes(b"elf with photo_read_transfer")
            self.assertTrue(accelerated.vault_source_required(pds))
            vault.write_bytes(b"elf with photo_read_transfer device_lock_transfer")
            self.assertFalse(accelerated.vault_source_required(pds))

    def test_device_lock_has_dedicated_vault_and_entropy_channels(self):
        system = ET.parse(ROOT / "experiments/hvf-smp/accelerated.system")
        self.assertIsNotNone(system.find("memory_region[@name='device_lock_transfer']"))
        for name in ("vault", "slint_ui"):
            self.assertIsNotNone(system.find(f"protection_domain[@name='{name}']/map[@mr='device_lock_transfer']"))
        channels = system.findall("channel")
        for peer, peer_id, ui_id in (("vault", "10", "15"), ("entropy_rng", "4", "16")):
            self.assertTrue(any(any(end.get("pd") == peer and end.get("id") == peer_id for end in channel)
                                and any(end.get("pd") == "slint_ui" and end.get("id") == ui_id and end.get("pp") == "true" for end in channel)
                                for channel in channels))


if __name__ == "__main__":
    unittest.main()
