#!/bin/sh
set -e

# Bundles the HyperHLE executable with the basic set of files needed for
# HyperHLE to run (the same ones found in the macOS .app bundle or Android APK).
# This does not prepare a full release.

if [[ $# == 1 ]]; then
    PATH_TO_BINARY="$1"
    shift

    rm -rf HyperHLE_windows_bundle
    mkdir HyperHLE_windows_bundle
    cp $PATH_TO_BINARY HyperHLE_windows_bundle/
    cp -r ../HyperHLE_dylibs HyperHLE_windows_bundle/
    cp -r ../HyperHLE_fonts HyperHLE_windows_bundle/
    cp -r ../HyperHLE_default_options.txt HyperHLE_windows_bundle/
else
    echo "Incorrect usage."
    exit 1
fi
