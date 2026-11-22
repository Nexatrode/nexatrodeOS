"""Check that the UTM display table follows host timezone rules."""
import datetime
import importlib.util
from pathlib import Path
import struct
import unittest
from zoneinfo import ZoneInfo

PATH = Path(__file__).resolve().parents[1] / 'experiments/slint-notes/timezone_table.py'
SPEC = importlib.util.spec_from_file_location('timezone_table', PATH)
timezone_table = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(timezone_table)


class TimezoneTableTest(unittest.TestCase):
    def test_utc_is_constant(self):
        self.assertEqual(timezone_table.make_table(ZoneInfo('UTC')), b'\0\0\0\0')

    def test_los_angeles_follows_future_daylight_saving(self):
        zone = ZoneInfo('America/Los_Angeles')
        table = timezone_table.make_table(zone)
        transitions = list(struct.iter_unpack('<Ii', table[4:]))
        for year in (2026, 2040, 2099):
            for month in (1, 4, 7, 11, 12):
                date = datetime.datetime(year, month, 15, 12, tzinfo=datetime.timezone.utc)
                epoch = int(date.timestamp())
                offset = struct.unpack('<i', table[:4])[0]
                for start, candidate in transitions:
                    if start > epoch:
                        break
                    offset = candidate
                self.assertEqual(offset, int(date.astimezone(zone).utcoffset().total_seconds()))


if __name__ == '__main__':
    unittest.main()
