# Windows Gaming

windows-gaming was a project that "officially" started exactly 5 years ago on the day that I'm writing this.
The goal was easy and straightforward GPU passthrough for Windows guests on Linux host systems.
Sadly, development stalled only one year later. With most of the basic functionality in place,
there was little justification for myself or the other contributors to spend time working
on features we no longer had any use for (e.g. easy setup, the project's original motivation).
Can't speak for the others, but it has always served me well in my daily use.

## Today's State

Over the last week, I have taken the old code and updated large parts of it to work with today's Rust ecosystem.
I have written a new guest agent in Rust. Major features include better shutdown (fixing a longstanding issue where windows would
blatantly ignore ACPI shutdown signals when the screen is off) and a new clipboard implementation
(the old X11 code does not work properly with XWayland under Wayland compositors).
Shoutout to @oberien, whose qmpgen idea is [reality](https://github.com/arcnmx/qapi-rs) nowadays.

### Roadmap

There is no roadmap. I made some changes which I sorely needed, and will continue to do so.
Working on a tool that benefits a broader audience is outside the scope of that and requires substantial work from other contributors.

### License

The original code was licensed under the Apache 2.0 license. The old `master` branch still exists, and you can find the license as well
as the original copyright holders there.

My modifications on the `main` branch, including some of my unreleased libraries (`zerocost-clipboard` etc) remain All Rights Reserved for now.
My changes are documented in detail using git commits.
If you would like to use the code (and hence need a different license), just open an issue and we can discuss.

-- @main--, 2021-11-18
