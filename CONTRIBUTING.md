# Contributing

Thanks for helping. cleanplate's rule is simple: **quality is measured, not claimed.**

## Before you open a pull request

1. `python3 tests/test_image.py` passes.
2. A change to the video method comes with ground-truth numbers from `research/score_gt.py` on the bench clips
   (build them with `research/synth.py`), before and after, plus a zoomed visual sheet (`research/sheet2.py`).
3. A change to the still method comes with `research/img_bench.py score` before and after.
4. A change that wins on numbers but looks worse, or the reverse, is reported as such. Rejected ideas are worth a line
   in `docs/benchmark.md` too: they save the next person a day.
5. Detection must stay safe: clean inputs are refused (the 5 clean video floors and the 9 clean still cases).

## Reporting a bad result

Open an issue with the "Bad result" template: the input (or a short crop of it), the command, the output, and which
generator and download resolution produced the file. If the file is private, a 2-second crop around the mark is enough.

## Scope

Visible marks only. Pull requests that touch SynthID, C2PA provenance or other invisible watermarks will be closed.

## Commits

Small, focused commits with a clear message. By contributing you agree your work is released under the MIT License.
