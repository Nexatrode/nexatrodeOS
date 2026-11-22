"""Generate display-only UTC offset transitions from the build host's timezone."""
import datetime
import struct
import sys
from zoneinfo import ZoneInfo


def make_table(zone):
    def offset(seconds):
        instant = datetime.datetime.fromtimestamp(seconds, datetime.timezone.utc)
        return int(instant.astimezone(zone).utcoffset().total_seconds())

    limit = 2**32 - 1  # The UTM PL031 RTC exposes 32-bit Unix seconds.
    previous = offset(0)
    result = bytearray(struct.pack('<i', previous))
    start = 0
    while start < limit:
        end = min(start + 12 * 3600, limit)
        current = offset(end)
        if current != previous:
            low, high = start, end
            while low + 1 < high:
                middle = (low + high) // 2
                if offset(middle) == previous:
                    low = middle
                else:
                    high = middle
            result.extend(struct.pack('<Ii', high, current))
            previous = current
        start = end
    return result


if __name__ == '__main__':
    with open('/etc/localtime', 'rb') as localtime:
        table = make_table(ZoneInfo.from_file(localtime))
    with open(sys.argv[1], 'wb') as output:
        output.write(table)
