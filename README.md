# BG3 mod order

A tool for extracting mod metadata and creating mod load order configuration.

# Features

* Supports mods using Laurian Studio's PAK format, v15-18.
* Mod load order support
* Auto detect mod paths on Linux and Windows (latter untested).
* Lightweight

# Why?

At the time, there were no working mod manager for Linux that didn't require Norbyte/lslib.
Instead of trying to get Norbyte/lslib to run I implemented the needed feature on my own in Rust.
