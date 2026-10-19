#!/bin/sh
# Build the fixtures macOS's own `textutil` writes, from committed sources:
#
#   textutil-memo.docx        Word, from textutil-memo.html            (P7.C1b1)
#   textutil-memo.cocoa.html  HTML, from that .docx                    (P7.C1c)
#   textutil-table.cocoa.html HTML, from textutil-table.html via RTF   (P7.C1c)
#
# The HTML ones are Apple's "Cocoa HTML Writer": class-based CSS with the
# `font` and `margin` shorthands, per-side borders and real <table> markup.
# Its DOCX writer drops tables, and HTML-to-HTML keeps the input as it is, so
# the table goes through RTF, which textutil writes tables into.
#
# The point is that VibePDF did not write it. textutil is Apple's DOCX writer:
# it uses direct formatting rather than styles, writes lists as literal
# "tab, bullet, tab" text, names the font "Times", and still emits the
# pre-standard `w:first-line` attribute — all things a reader tested only
# against its own writer would never see. Run once on macOS and commit:
#
#     sh tests/fixtures/basic/generate-textutil-docx.sh
set -eu
cd "$(dirname "$0")"
textutil -convert docx -output textutil-memo.docx textutil-memo.html
echo "wrote $(pwd)/textutil-memo.docx"
textutil -convert html -output textutil-memo.cocoa.html textutil-memo.docx
echo "wrote $(pwd)/textutil-memo.cocoa.html"
tmp=$(mktemp -d)
textutil -convert rtf -output "$tmp/table.rtf" textutil-table.html
textutil -convert html -output textutil-table.cocoa.html "$tmp/table.rtf"
rm -r "$tmp"
echo "wrote $(pwd)/textutil-table.cocoa.html"
