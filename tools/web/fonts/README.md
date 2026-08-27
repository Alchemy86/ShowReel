Montserrat (Julieta Ulanovsky et al.) and Open Sans (Steve Matteson), both
licensed under the SIL Open Font License 1.1, which permits bundling and
redistribution. These are the same two families `src/theme.rs`'s `DISPLAY`
and `BODY` stacks ask for first; the native binary finds them if they happen
to be installed system-wide, but a browser has no system font directory to
scan, so the web build ships the weights the default theme actually uses
(regular, semibold/600, and the display face's extra-bold/800 — see the
weights in `src/theme.rs`) instead.
