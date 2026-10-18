#!/bin/sh
# Build textutil-memo.docx from textutil-memo.html with macOS's own `textutil`.
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
