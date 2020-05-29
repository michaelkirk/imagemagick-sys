# imagemagick-sys

🚨🚨 __This is a work in progress and at this point should not be considered
safe or reliable. Currently only building for macOS.__ 🚨🚨

Rust crate for linking to the [ImageMagick 7](https://imagemagick.org/index.php) library.

If an appropriate version of ImageMagick can be found already installed, this
crate will attempt to link to it, otherwise it will build from source.

If you do have ImageMagick installed on your system, but for some reason you'd prefer
to force this crate to build it from source, use the `static` feature or
specify the environment variable `IMAGEMAGICK_SYS_STATIC`.
