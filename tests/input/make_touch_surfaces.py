#!/usr/bin/env python3
"""Render the input-spike's visible touch targets without external fonts."""

from pathlib import Path
import sys

WIDTH = 300
HEIGHT = 400
FONT = {
    "0": ("01110", "10001", "10011", "10101", "11001", "10001", "01110"),
    "1": ("00100", "01100", "00100", "00100", "00100", "00100", "01110"),
    "2": ("01110", "10001", "00001", "00010", "00100", "01000", "11111"),
    "3": ("11110", "00001", "00001", "01110", "00001", "00001", "11110"),
    "4": ("00010", "00110", "01010", "10010", "11111", "00010", "00010"),
    "5": ("11111", "10000", "10000", "11110", "00001", "00001", "11110"),
    "6": ("01110", "10000", "10000", "11110", "10001", "10001", "01110"),
    "7": ("11111", "00001", "00010", "00100", "01000", "01000", "01000"),
    "8": ("01110", "10001", "10001", "01110", "10001", "10001", "01110"),
    "9": ("01110", "10001", "10001", "01111", "00001", "00001", "01110"),
    "A": ("01110", "10001", "10001", "11111", "10001", "10001", "10001"),
    "B": ("11110", "10001", "10001", "11110", "10001", "10001", "11110"),
    "C": ("01111", "10000", "10000", "10000", "10000", "10000", "01111"),
    "D": ("11110", "10001", "10001", "10001", "10001", "10001", "11110"),
    "E": ("11111", "10000", "10000", "11110", "10000", "10000", "11111"),
    "F": ("11111", "10000", "10000", "11110", "10000", "10000", "10000"),
    "G": ("01111", "10000", "10000", "10111", "10001", "10001", "01111"),
    "H": ("10001", "10001", "10001", "11111", "10001", "10001", "10001"),
    "I": ("11111", "00100", "00100", "00100", "00100", "00100", "11111"),
    "J": ("00111", "00010", "00010", "00010", "10010", "10010", "01100"),
    "K": ("10001", "10010", "10100", "11000", "10100", "10010", "10001"),
    "L": ("10000", "10000", "10000", "10000", "10000", "10000", "11111"),
    "M": ("10001", "11011", "10101", "10101", "10001", "10001", "10001"),
    "N": ("10001", "11001", "10101", "10011", "10001", "10001", "10001"),
    "O": ("01110", "10001", "10001", "10001", "10001", "10001", "01110"),
    "P": ("11110", "10001", "10001", "11110", "10000", "10000", "10000"),
    "Q": ("01110", "10001", "10001", "10001", "10101", "10010", "01101"),
    "R": ("11110", "10001", "10001", "11110", "10100", "10010", "10001"),
    "S": ("01111", "10000", "10000", "01110", "00001", "00001", "11110"),
    "T": ("11111", "00100", "00100", "00100", "00100", "00100", "00100"),
    "U": ("10001", "10001", "10001", "10001", "10001", "10001", "01110"),
    "V": ("10001", "10001", "10001", "10001", "10001", "01010", "00100"),
    "W": ("10001", "10001", "10001", "10101", "10101", "10101", "01010"),
    "X": ("10001", "10001", "01010", "00100", "01010", "10001", "10001"),
    "Y": ("10001", "10001", "01010", "00100", "00100", "00100", "00100"),
    "Z": ("11111", "00001", "00010", "00100", "01000", "10000", "11111"),
    "-": ("00000", "00000", "00000", "11111", "00000", "00000", "00000"),
}


class Frame:
    def __init__(self):
        self.pixels = bytearray(WIDTH * HEIGHT * 3)
        self.rect(0, 0, WIDTH, HEIGHT, (11, 21, 34))

    def rect(self, x, y, width, height, color):
        row = bytes(color) * width
        for py in range(max(0, y), min(HEIGHT, y + height)):
            start = (py * WIDTH + max(0, x)) * 3
            end = (py * WIDTH + min(WIDTH, x + width)) * 3
            self.pixels[start:end] = row[:end - start]

    def border(self, x, y, width, height, color, thickness=2):
        self.rect(x, y, width, thickness, color)
        self.rect(x, y + height - thickness, width, thickness, color)
        self.rect(x, y, thickness, height, color)
        self.rect(x + width - thickness, y, thickness, height, color)

    def text(self, x, y, value, color, scale=1):
        cursor = x
        for character in value:
            glyph = FONT.get(character)
            if glyph:
                for row, bits in enumerate(glyph):
                    for column, bit in enumerate(bits):
                        if bit == "1":
                            self.rect(cursor + column * scale, y + row * scale,
                                      scale, scale, color)
            cursor += 6 * scale

    def circle(self, cx, cy, radius, color):
        for y in range(-radius, radius + 1):
            half_width = int((radius * radius - y * y) ** 0.5)
            self.rect(cx - half_width, cy + y, half_width * 2 + 1, 1, color)

    def save(self, path):
        Path(path).write_bytes(f"P6\n{WIDTH} {HEIGHT}\n255\n".encode() + self.pixels)


def main():
    if len(sys.argv) < 2 or len(sys.argv) > 10:
        raise SystemExit(
            "usage: make_touch_surfaces.py OUTPUT.ppm "
            "[none|notes|contacts|calculator|clock|calendar|messages|phone|settings] [APP_TEXT] "
            "[edit|list] [NOTE_COUNT] [SELECTED_NOTE] [SCROLL_START_LINE] [DELETE_ARMED] "
            "[body|title]"
        )
    focus = sys.argv[2] if len(sys.argv) >= 3 else "none"
    app_text = sys.argv[3] if len(sys.argv) >= 4 else ""
    notes_mode = "edit"
    note_count = 1
    selected_note = 0
    requested_scroll_start = None
    delete_armed = False
    notes_field = "body"
    if len(sys.argv) >= 5:
        if sys.argv[4] in ("edit", "list"):
            notes_mode = sys.argv[4]
            note_count = int(sys.argv[5]) if len(sys.argv) >= 6 else 1
            selected_note = int(sys.argv[6]) if len(sys.argv) >= 7 else 0
            requested_scroll_start = int(sys.argv[7]) if len(sys.argv) >= 8 else None
            delete_armed = bool(int(sys.argv[8])) if len(sys.argv) >= 9 else False
            notes_field = sys.argv[9] if len(sys.argv) >= 10 else "body"
        else:
            requested_scroll_start = int(sys.argv[4])
    if focus not in ("none", "notes", "contacts", "calculator", "clock", "calendar",
                     "messages", "phone", "settings"):
        raise SystemExit(
            "focus must be none, notes, contacts, calculator, clock, calendar, "
            "messages, phone, or settings"
        )
    if len(app_text) > 128 or any(character not in "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 .\n-" for character in app_text):
        raise SystemExit("APP_TEXT must contain at most 128 uppercase letters, digits, spaces, periods, hyphens, or newlines")
    if note_count < 1 or note_count > 4 or selected_note < 0 or selected_note >= note_count:
        raise SystemExit("note count and selection are outside the four-note limit")
    if notes_field not in ("body", "title"):
        raise SystemExit("notes field must be body or title")

    frame = Frame()
    frame.rect(0, 0, WIDTH, 3, (105, 224, 193))
    frame.text(20, 8, "PERSONAL OS", (232, 242, 245), scale=2)
    frame.text(20, 38, "TOUCH A WORKSPACE", (145, 168, 183))

    cards = (
        ("notes", 12, 50, 135, 44, (24, 57, 80), (105, 224, 193),
         "NOTES", "PRIVATE NOTES", 2),
        ("contacts", 153, 50, 135, 44, (41, 48, 79), (169, 155, 255),
         "CONTACTS", "PEOPLE DETAILS", 2),
        ("calculator", 12, 98, 135, 44, (39, 48, 79), (255, 206, 84),
         "CALCULATOR", "FOUR FUNCTION", 2),
        ("clock", 153, 98, 135, 44, (32, 49, 65), (131, 241, 193),
         "CLOCK", "UTC TIME", 2),
        ("calendar", 12, 146, 135, 44, (48, 42, 58), (255, 168, 120),
         "CALENDAR", "REMINDERS", 2),
        ("messages", 153, 146, 135, 44, (42, 52, 48), (120, 220, 180),
         "MESSAGES", "EXPLICIT KEYS", 2),
        ("phone", 12, 194, 135, 26, (52, 42, 38), (255, 168, 120),
         "PHONE", "FAKE MODEM", 1),
        ("settings", 153, 194, 135, 26, (38, 44, 52), (160, 180, 200),
         "SETTINGS", "LOCAL ONLY", 1),
    )
    for name, x, y, width, height, fill, border, title, subtitle, scale in cards:
        frame.rect(x, y, width, height, fill)
        frame.border(x, y, width, height, border)
        title_width = len(title) * 6 * scale
        frame.text(x + (width - title_width) // 2, y + (8 if height > 40 else 4),
                   title, (240, 251, 250), scale=scale)
        subtitle_width = len(subtitle) * 6
        frame.text(x + (width - subtitle_width) // 2, y + (height - 16 if height > 40 else height - 12),
                   subtitle, (189, 212, 216))

    frame.rect(12, 222, 276, 140, (18, 38, 53))
    frame.border(12, 222, 276, 140, (51, 73, 89))

    def keycap(x, y, width, label, height=27):
        frame.rect(x, y, width, height, (32, 58, 75))
        frame.border(x, y, width, height, (74, 101, 117), thickness=1)
        frame.text(x + max(2, (width - len(label) * 6) // 2),
                   y + (height - 7) // 2,
                   label, (232, 242, 245))

    for index, digit in enumerate("1234567890"):
        keycap(16 + index * 27, 226, 24, digit, height=20)
    for index, letter in enumerate("QWERTYUIOP"):
        keycap(16 + index * 27, 248, 24, letter, height=20)
    for index, letter in enumerate("ASDFGHJKL"):
        keycap(29 + index * 27, 270, 24, letter, height=20)
    for index, letter in enumerate("ZXCVBNM"):
        keycap(56 + index * 27, 292, 24, letter, height=20)
    keycap(20, 324, 84, "SPACE")
    keycap(108, 324, 76, "ENTER")
    keycap(188, 324, 95, "BACKSPACE")

    if focus == "notes":
        frame.border(12, 50, 135, 44, (255, 206, 84), thickness=4)
    elif focus == "contacts":
        frame.border(153, 50, 135, 44, (255, 206, 84), thickness=4)
    elif focus == "calculator":
        frame.border(12, 98, 135, 44, (255, 206, 84), thickness=4)
    elif focus == "clock":
        frame.border(153, 98, 135, 44, (255, 206, 84), thickness=4)
    elif focus == "calendar":
        frame.border(12, 146, 135, 44, (255, 206, 84), thickness=4)
    elif focus == "messages":
        frame.border(153, 146, 135, 44, (255, 206, 84), thickness=4)
    elif focus == "phone":
        frame.border(12, 194, 135, 26, (255, 206, 84), thickness=4)
    elif focus == "settings":
        frame.border(153, 194, 135, 26, (255, 206, 84), thickness=4)

    if focus != "none":
        frame.rect(0, 0, WIDTH, 222, (11, 21, 34))
        frame.rect(0, 0, WIDTH, 3, (105, 224, 193))

        def nav_button(x, width, label, selected, home=False):
            fill = (24, 57, 80) if home or selected else (41, 48, 79)
            border = (255, 206, 84) if selected else (
                (105, 224, 193) if home else (169, 155, 255)
            )
            frame.rect(x, 12, width, 32, fill)
            frame.border(x, 12, width, 32, border, thickness=2)
            frame.text(x + 8, 23, label, (232, 242, 245))

        nav_button(12, 32, "HOME", False, home=True)
        nav_button(46, 24, "NT", focus == "notes")
        nav_button(72, 24, "CT", focus == "contacts")
        nav_button(98, 24, "CA", focus == "calculator")
        nav_button(124, 24, "CK", focus == "clock")
        nav_button(150, 24, "CL", focus == "calendar")
        nav_button(176, 24, "MS", focus == "messages")
        nav_button(202, 24, "PH", focus == "phone")
        nav_button(228, 24, "ST", focus == "settings")

        if focus == "notes":
            if notes_mode == "list":
                frame.text(18, 58, "MY NOTES", (232, 242, 245), scale=2)
                subtitle = "TAP DELETE AGAIN" if delete_armed else "TAP A NOTE TO OPEN"
                subtitle_color = (131, 241, 193) if delete_armed else (189, 212, 216)
                frame.text(20, 82, subtitle, subtitle_color)
            else:
                frame.text(18, 58, "PRIVATE NOTES", (232, 242, 245), scale=2)
                field_name = "TITLE" if notes_field == "title" else "BODY"
                frame.text(20, 82, f"{field_name} {selected_note + 1} OF {note_count}", (189, 212, 216))
            field_label = "NOTE TEXT"
            field_color = (189, 212, 216)
            field_border = (105, 224, 193)
        elif focus == "contacts":
            frame.text(18, 58, "CONTACTS", (244, 241, 255), scale=2)
            frame.text(20, 82, "PEOPLE AND DETAILS", (208, 204, 228))
            field_label = "CONTACT LIST"
            field_color = (208, 204, 228)
            field_border = (169, 155, 255)
        elif focus == "calendar":
            frame.text(18, 58, "CALENDAR", (255, 236, 224), scale=2)
            frame.text(20, 82, "LOCAL REMINDERS", (228, 204, 189))
            field_label = "REMINDERS"
            field_color = (228, 204, 189)
            field_border = (255, 168, 120)
        elif focus == "messages":
            frame.text(18, 58, "MESSAGES", (224, 255, 240), scale=2)
            frame.text(20, 82, "EXPLICIT CONTACT ONLY", (189, 228, 208))
            field_label = "PEER KEYS"
            field_color = (189, 228, 208)
            field_border = (120, 220, 180)
        elif focus == "phone":
            frame.text(18, 58, "PHONE", (255, 240, 224), scale=2)
            frame.text(20, 82, "FAKE CARRIER", (228, 208, 189))
            field_label = "STATUS"
            field_color = (228, 208, 189)
            field_border = (255, 168, 120)
        elif focus == "settings":
            frame.text(18, 58, "SETTINGS", (232, 242, 245), scale=2)
            frame.text(20, 82, "LOCAL DEVICE OPTIONS", (189, 212, 216))
            field_label = "STATUS"
            field_color = (189, 212, 216)
            field_border = (160, 180, 200)
        else:
            if focus == "calculator":
                frame.text(18, 58, "CALCULATOR", (232, 242, 245), scale=2)
                frame.text(20, 82, "FOUR FUNCTION INTEGER", (189, 212, 216))
                field_label = "RESULT"
                field_color = (189, 212, 216)
                field_border = (255, 206, 84)
            else:
                frame.text(18, 58, "CLOCK", (232, 242, 245), scale=2)
                frame.text(20, 82, "UTC DATE AND TIME FROM RTC", (189, 212, 216))
                field_label = "UTC DATE"
                field_color = (131, 241, 193)
                field_border = (131, 241, 193)

        frame.rect(12, 105, 276, 89, (18, 38, 53))
        frame.border(12, 105, 276, 89, field_border, thickness=2)
        if focus == "notes" and notes_mode == "list":
            field_label = "SAVED NOTES"
        elif focus == "notes" and notes_field == "title":
            field_label = "TITLE"
        frame.text(24, 114, field_label, field_color)
        if focus in ("notes", "contacts", "calendar", "messages", "phone", "settings"):
            if focus == "notes" and notes_mode == "list":
                lines = app_text.split("\n") if app_text else []
                for index, line_text in enumerate(lines[:note_count]):
                    row_y = 130 + index * 15
                    if index == selected_note:
                        frame.rect(20, row_y, 260, 14, (24, 57, 80))
                        frame.border(20, row_y, 260, 14, (255, 206, 84), thickness=1)
                    frame.text(24, row_y + 3, line_text[:21], field_color)
                cursor_x = cursor_y = -1
            elif focus == "notes" and notes_field == "title":
                frame.text(24, 137, app_text, field_color, scale=2)
                cursor_x = 24 + len(app_text) * 12
                cursor_y = 136 if len(app_text) <= 21 else -1
            else:
                total_lines = 1
                column = 0
                for character in app_text:
                    if character == "\n":
                        total_lines += 1
                        column = 0
                    else:
                        if column == 21:
                            total_lines += 1
                            column = 0
                        column += 1
                maximum_first_line = max(0, total_lines - 3)
                first_line = maximum_first_line if requested_scroll_start is None else min(
                    max(0, requested_scroll_start), maximum_first_line
                )
                line = 0
                column = 0
                for character in app_text:
                    if character == "\n":
                        line += 1
                        column = 0
                        continue
                    if column == 21:
                        line += 1
                        column = 0
                    if first_line <= line < first_line + 3:
                        frame.text(24 + column * 12, 137 + (line - first_line) * 18,
                                   character, field_color, scale=2)
                    column += 1
                if column == 21:
                    line += 1
                    column = 0
                cursor_visible = first_line <= line < first_line + 3
                cursor_x = 24 + column * 12
                cursor_y = 136 + (line - first_line) * 18 if cursor_visible else -1
        elif focus == "calculator":
            frame.text(24, 143, app_text, field_color, scale=2)
            cursor_x = cursor_y = -1
        elif focus == "clock":
            if "\n" in app_text:
                date_text, time_text = app_text.split("\n", 1)
            else:
                date_text, time_text = app_text, ""
            frame.text(24, 132, date_text, field_color, scale=2)
            frame.text(24, 156, "UTC TIME", field_color)
            frame.text(102, 174, time_text, field_color, scale=2)
            cursor_x = cursor_y = -1
        else:
            if app_text:
                frame.text(24, 137, app_text, field_color, scale=2)
            cursor_x = 24 + len(app_text) * 12
            cursor_y = 136
        if cursor_y >= 0:
            frame.rect(cursor_x, cursor_y, 2, 16, (131, 241, 193))
        if focus == "notes":
            positions = ((20, 48), (72, 48), (124, 48), (176, 48), (228, 55))
            labels = ("OPEN", "UP", "DOWN", "NEW", "DELETE") if notes_mode == "list" else (
                "LIST", "TITLE", "BODY", "OLDER", "NEWER"
            )
            if notes_mode == "edit" and notes_field == "title":
                labels = ("LIST", "TITLE", "BODY", "", "")
            for (x, width), label in zip(positions, labels):
                enabled = bool(label)
                fill = (32, 58, 75) if enabled else (43, 49, 52)
                border = (74, 101, 117) if enabled else (61, 76, 83)
                frame.rect(x, 198, width, 20, fill)
                frame.border(x, 198, width, 20, border, thickness=1)
                if label:
                    frame.text(x + (width - len(label) * 6) // 2, 204,
                               label, (232, 242, 245))
        elif focus == "calculator":
            frame.text(23, 204, "F1 ADD  F2 SUB  ENTER EQUALS", (139, 168, 190))
            frame.rect(12, 222, 276, 140, (18, 38, 53))
            frame.border(12, 222, 276, 140, (51, 73, 89), thickness=2)
            keypad_rows = (("7", "8", "9", "DIV"),
                          ("4", "5", "6", "MUL"),
                          ("1", "2", "3", "SUB"),
                          ("0", "DEL", "CLR", "ADD"))
            for row, labels in enumerate(keypad_rows):
                for column, label in enumerate(labels):
                    x = 16 + column * 67
                    y = 226 + row * 26
                    frame.rect(x, y, 60, 22, (32, 58, 75))
                    frame.border(x, y, 60, 22, (74, 101, 117), thickness=1)
                    frame.text(x + (60 - len(label) * 6) // 2, y + 7,
                               label, (232, 242, 245))
            frame.rect(16, 330, 261, 22, (32, 58, 75))
            frame.border(16, 330, 261, 22, (74, 101, 117), thickness=1)
            frame.text(131, 337, "ENTER", (232, 242, 245))
        elif focus == "clock":
            frame.text(20, 204, "LIVE UTC DATE AND TIME", (139, 184, 190))
        elif focus == "messages":
            if app_text.startswith("NO THREADS"):
                frame.text(20, 204, "ENTER ADD A PEER KEY", (139, 168, 190))
            elif app_text.startswith("ADD KEY"):
                frame.text(20, 204, "HEX 0-9 A-F  ENTER AT 8", (139, 168, 190))
            elif app_text.startswith("COMPOSE KEY"):
                frame.text(20, 204, "A-Z BODY  ENTER SEAL", (139, 168, 190))
            else:
                def control(x, width, label):
                    frame.rect(x, 198, width, 20, (32, 58, 75))
                    frame.border(x, 198, width, 20, (74, 101, 117), thickness=1)
                    frame.text(x + max(2, (width - len(label) * 6) // 2), 204,
                               label, (232, 242, 245))

                control(20, 84, "UP")
                control(108, 76, "DOWN")
                control(188, 95, "DELETE")
        elif focus == "settings":
            frame.text(20, 204, "LOCAL ONLY  NO NET YET", (139, 184, 190))
        elif focus == "phone":
            frame.text(20, 204, "DEL CLR  ENTER CALL", (139, 168, 190))
            frame.rect(12, 222, 276, 140, (18, 38, 53))
            frame.border(12, 222, 276, 140, (51, 73, 89), thickness=2)
            keypad_rows = (("7", "8", "9", ""),
                          ("4", "5", "6", ""),
                          ("1", "2", "3", ""),
                          ("0", "DEL", "CLR", ""))
            for row, labels in enumerate(keypad_rows):
                for column, label in enumerate(labels):
                    if not label:
                        continue
                    x = 16 + column * 67
                    y = 226 + row * 26
                    frame.rect(x, y, 60, 22, (32, 58, 75))
                    frame.border(x, y, 60, 22, (74, 101, 117), thickness=1)
                    frame.text(x + (60 - len(label) * 6) // 2, y + 7,
                               label, (232, 242, 245))
            frame.rect(16, 330, 261, 22, (32, 58, 75))
            frame.border(16, 330, 261, 22, (74, 101, 117), thickness=1)
            frame.text(134, 337, "CALL", (232, 242, 245))
        elif app_text.startswith("ADD "):
            frame.text(20, 204, "ENTER NEXT  BACKSPACE EDIT", (139, 168, 190))
        elif app_text.startswith("NO CONTACTS") or app_text.startswith("NO REMINDERS"):
            hint = "ENTER ADD A CONTACT" if focus == "contacts" else "ENTER ADD A REMINDER"
            frame.text(20, 204, hint, (139, 168, 190))
        else:
            def control(x, width, label):
                frame.rect(x, 198, width, 20, (32, 58, 75))
                frame.border(x, 198, width, 20, (74, 101, 117), thickness=1)
                frame.text(x + max(2, (width - len(label) * 6) // 2), 204,
                           label, (232, 242, 245))

            control(20, 84, "UP")
            control(108, 76, "DOWN")
            control(188, 95, "DELETE")

    frame.rect(20, 374, 260, 1, (51, 73, 89))
    frame.text(22, 385, "SECURE INPUT ROUTER", (120, 144, 160))

    # Guest-drawn tablet cursor parked in the footer for stable QEMU compares.
    # Amber arrow with dark halo (matches tests/input/gpu_client.c).
    pointer_x, pointer_y = 290, 390
    cursor = (
        "X      ",
        "XX     ",
        "X X    ",
        "X  X   ",
        "X   X  ",
        "X    X ",
        "XXXXX  ",
        "X  X   ",
        "X   X  ",
        "     X ",
    )
    for row, pattern in enumerate(cursor):
        for column, cell in enumerate(pattern):
            if cell != "X":
                continue
            px = pointer_x + column
            py = pointer_y + row
            for oy in (-1, 0, 1):
                for ox in (-1, 0, 1):
                    if ox == 0 and oy == 0:
                        continue
                    hx, hy = px + ox, py + oy
                    if 0 <= hx < WIDTH and 0 <= hy < HEIGHT:
                        frame.rect(hx, hy, 1, 1, (11, 21, 18))
            if 0 <= px < WIDTH and 0 <= py < HEIGHT:
                frame.rect(px, py, 1, 1, (255, 206, 20))

    frame.save(sys.argv[1])


if __name__ == "__main__":
    main()
