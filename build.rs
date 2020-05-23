extern crate pkg_config;

use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

const MIN_VERSION: &'static str = "7.0";
const MAX_VERSION: &'static str = "7.1";

fn main() -> Result<(), Error> {
    println!("cargo:rerun-if-env-changed=IMAGEMAGICK_SYS_STATIC");
    let wants_static = cfg!(feature = "static")
        || env::var("IMAGEMAGICK_SYS_STATIC").unwrap_or(String::new()) == "1";

    if wants_static {
        eprintln!("wants static");
        build_from_source()?;
    } else if let Ok(true) = find_existing_installation() {
        if let Ok(val) = env::var("_TEST_EXPECT_BUILD_FROM_SOURCE") {
            if val != "0" {
                println!("cargo:warning=for testing purposes: package was not built from source but it should have been");
                exit(1);
            }
        }
        eprintln!("found existing installation.");
    } else {
        // If no existing installation was found, fall back to building from source
        eprintln!("no existing installation found, building from source.");
        build_from_source()?;
    }

    Ok(())
}

fn find_existing_installation() -> Result<bool, Error> {
    if cfg!(target_os = "freebsd") {
        // pkg_config does not seem to work properly on FreeBSD, so
        // hard-code the builder settings for the time being.
        env_var_set_default(
            "IMAGE_MAGICK_INCLUDE_DIRS",
            "/usr/local/include/ImageMagick-7",
        );
        // Need to hack the linker flags as well.
        env_var_set_default("IMAGE_MAGICK_LIB_DIRS", "/usr/local/lib");
        env_var_set_default("IMAGE_MAGICK_LIBS", "MagickWand-7");
    }

    let lib_dirs = find_image_magick_lib_dirs()?;
    for d in &lib_dirs {
        if !d.exists() {
            panic!(
                "ImageMagick library directory does not exist: {}",
                d.to_string_lossy()
            );
        }
        println!("cargo:rustc-link-search=native={}", d.to_string_lossy());
    }
    let include_dirs = find_image_magick_include_dirs()?;
    for d in &include_dirs {
        if !d.exists() {
            panic!(
                "ImageMagick include directory does not exist: {}",
                d.to_string_lossy()
            );
        }
        println!("cargo:include={}", d.to_string_lossy());
    }
    println!("cargo:rerun-if-env-changed=IMAGE_MAGICK_LIBS");

    let target = env::var("TARGET").unwrap();
    let libs_env = env::var("IMAGE_MAGICK_LIBS").ok();
    let libs = match libs_env {
        Some(ref v) => v.split(":").map(|x| x.to_owned()).collect(),
        None => {
            if target.contains("windows") {
                vec!["CORE_RL_MagickWand_".to_string()]
            } else if target.contains("freebsd") {
                vec!["MagickWand-7".to_string()]
            } else {
                run_pkg_config()?.libs
            }
        }
    };

    let kind = determine_mode(&lib_dirs, libs.as_slice());
    for lib in libs.into_iter() {
        println!("cargo:rustc-link-lib={}={}", kind, lib);
    }

    Ok(true)
}

fn env_var_set_default(name: &str, value: &str) {
    if env::var(name).is_err() {
        env::set_var(name, value);
    }
}

fn find_image_magick_lib_dirs() -> Result<Vec<PathBuf>, Error> {
    println!("cargo:rerun-if-env-changed=IMAGE_MAGICK_LIB_DIRS");
    env::var("IMAGE_MAGICK_LIB_DIRS")
        .map(|x| x.split(":").map(PathBuf::from).collect::<Vec<PathBuf>>())
        .or_else(|_| Ok(vec![find_image_magick_dir()?.join("lib")]))
        .or_else(|_: env::VarError| -> Result<_, Error> { Ok(run_pkg_config()?.link_paths) })
}

fn find_image_magick_include_dirs() -> Result<Vec<PathBuf>, Error> {
    println!("cargo:rerun-if-env-changed=IMAGE_MAGICK_INCLUDE_DIRS");
    env::var("IMAGE_MAGICK_INCLUDE_DIRS")
        .map(|x| x.split(":").map(PathBuf::from).collect::<Vec<PathBuf>>())
        .or_else(|_| Ok(vec![find_image_magick_dir()?.join("include")]))
        .or_else(|_: env::VarError| -> Result<_, Error> { Ok(run_pkg_config()?.include_paths) })
}

fn find_image_magick_dir() -> Result<PathBuf, env::VarError> {
    println!("cargo:rerun-if-env-changed=IMAGE_MAGICK_DIR");
    env::var("IMAGE_MAGICK_DIR").map(PathBuf::from)
}

fn determine_mode<T: AsRef<str>>(libdirs: &Vec<PathBuf>, libs: &[T]) -> &'static str {
    println!("cargo:rerun-if-env-changed=IMAGE_MAGICK_STATIC");
    let kind = env::var("IMAGE_MAGICK_STATIC").ok();
    match kind.as_ref().map(|s| &s[..]) {
        Some("0") => return "dylib",
        Some(_) => return "static",
        None => {}
    }

    // See what files we actually have to link against, and see what our
    // possibilities even are.
    let files = libdirs
        .into_iter()
        .flat_map(|d| d.read_dir().unwrap())
        .map(|e| e.unwrap())
        .map(|e| e.file_name())
        .filter_map(|e| e.into_string().ok())
        .collect::<HashSet<_>>();
    let can_static = libs.iter().all(|l| {
        files.contains(&format!("lib{}.a", l.as_ref()))
            || files.contains(&format!("{}.lib", l.as_ref()))
    });
    let can_dylib = libs.iter().all(|l| {
        files.contains(&format!("lib{}.so", l.as_ref()))
            || files.contains(&format!("{}.dll", l.as_ref()))
            || files.contains(&format!("lib{}.dylib", l.as_ref()))
    });

    match (can_static, can_dylib) {
        (true, false) => return "static",
        (false, true) => return "dylib",
        (false, false) => {
            panic!(
                "ImageMagick libdirs at `{:?}` do not contain the required files \
                 to either statically or dynamically link ImageMagick",
                libdirs
            );
        }
        (true, true) => {}
    }

    // default
    "dylib"
}

fn run_pkg_config() -> Result<pkg_config::Library, Error> {
    // Assert that the appropriate version of MagickWand is installed,
    // since we are very dependent on the particulars of MagickWand.
    pkg_config::Config::new()
        .cargo_metadata(false)
        .atleast_version(MIN_VERSION)
        .probe("MagickWand")?;

    // Check the maximum version separately as pkg-config will ignore that
    // option when combined with (certain) other options. And since the
    // pkg-config crate always adds those other flags, we must run the
    // command directly.
    if !Command::new("pkg-config")
        .arg(format!("--max-version={}", MAX_VERSION))
        .arg("MagickWand")
        .status()
        .unwrap()
        .success()
    {
        panic!(format!(
            "MagickWand version must be no higher than {}",
            MAX_VERSION
        ));
    }

    // We have to split the version check and the cflags/libs check because
    // you can't do both at the same time on RHEL (apparently).
    Ok(pkg_config::Config::new()
        .cargo_metadata(false)
        .probe("MagickWand")?)
}

fn build_from_source() -> Result<(), Error> {
    if let Ok(val) = &env::var("_TEST_EXPECT_USE_EXISTING_INSTALLATION") {
        if val != "0" {
            println!("cargo:warning=for testing purposes: package was building from source but should not have been");
            exit(1);
        }
    }

    let out_dir = env::var("OUT_DIR").unwrap();
    let num_jobs = env::var("NUM_JOBS");

    let cmd_path = std::fs::canonicalize("imagemagick-src/configure").unwrap();

    // helpful while testing linking options
    let skip_build = false;
    if skip_build {
        eprintln!("skipping build");
    } else {
        let mut configure_cmd = Command::new(cmd_path);
        configure_cmd
            .current_dir(Path::new("./imagemagick-src"))
            .arg("--disable-osx-universal-binary")
            .arg("--with-magick-plus-plus=no")
            .arg("--with-perl=no")
            .arg("--disable-dependency-tracking")
            .arg("--disable-silent-rules")
            .arg("--disable-opencl")
            .arg("--disable-shared")
            .arg("--enable-static")
            .arg("--with-freetype=yes")
            .arg("--with-modules")
            .arg("--with-openjp2")
            .arg("--with-openexr")
            .arg("--with-webp=yes")
            .arg("--with-heic=no")
            .arg("--with-gslib")
            .arg("--without-fftw")
            .arg("--without-pango")
            .arg("--without-x")
            .arg("--without-wmf")
            .arg("--prefix")
            .arg(&out_dir);

        eprintln!("running `configure`...");
        match configure_cmd.output() {
            Ok(out) => {
                if !out.status.success() {
                    eprintln!(
                        "`configure` failed:\nstdout:\n{}\nstderr:\n{}",
                        String::from_utf8(out.stdout).unwrap(),
                        String::from_utf8(out.stderr).unwrap()
                    );
                    exit(1);
                }
            }
            Err(e) => {
                eprintln!("`configure` command execution failed: {:?}", e);
                exit(1)
            }
        }

        eprintln!("running `make install`...");
        let mut make_cmd = Command::new("make");
        make_cmd
            .current_dir(Path::new("imagemagick-src"))
            .arg("install");

        if let Ok(jobs) = num_jobs {
            make_cmd.arg(format!("-j{}", jobs));
        }

        match make_cmd.output() {
            Ok(out) => {
                if !out.status.success() {
                    eprintln!(
                        "`make install` failed:\nstdout:\n{}\nstderr:\n{}",
                        String::from_utf8(out.stdout).unwrap(),
                        String::from_utf8(out.stderr).unwrap()
                    );
                    exit(1)
                }
            }
            Err(e) => {
                eprintln!("`make install` command execution failed: {:?}", e);
                exit(1)
            }
        }
        eprintln!("finished `make install`");
    }

    // At this point, in most sys crates, we'd link the compiled libs doing something like:
    //
    //     println!("cargo:rustc-link-search=native={}/lib", &out_dir);
    //     println!("cargo:rustc-link-lib=MagickCore");
    //     println!("cargo:rustc-link-lib=MagickWand");
    //     println!("cargo:include={}/include", &out_dir);
    //
    // However, ImageMagick outputs libraries with a different name depending on the features
    // enabled. For example something like: "MagickWand-7.Q16HDRI", which means, that it's the
    // MagickWand lib, version 7, compiled with quantum depth of 16, with the HDRI feature enabled.
    //
    // Changing any of those features changes the name of the lib. Rather than try to track which
    // features are enabled and then reverse engineering the lib name from that, it seems more
    // resilient to point pkg_config at the just-compiled lib and query the pkg_config metadata for
    // the lib names directly.

    let previous_value = env::var("PKG_CONFIG_PATH");
    env::set_var("PKG_CONFIG_PATH", format!("{}/lib/pkgconfig", &out_dir));
    let config = pkg_config::Config::new()
        .cargo_metadata(true)
        .statik(true)
        .probe("MagickWand")?;
    // restore env
    if let Ok(previous_value) = previous_value {
        env::set_var("PKG_CONFIG_PATH", previous_value);
    }

    for d in config.include_paths {
        println!("cargo:include={}", d.to_string_lossy());
    }

    Ok(())
}

#[derive(Debug)]
enum Error {
    Wrapped(Box<dyn std::error::Error>),
}

impl std::convert::From<pkg_config::Error> for Error {
    fn from(e: pkg_config::Error) -> Self {
        Error::Wrapped(Box::new(e))
    }
}

impl std::convert::From<std::env::VarError> for Error {
    fn from(e: std::env::VarError) -> Self {
        Error::Wrapped(Box::new(e))
    }
}
