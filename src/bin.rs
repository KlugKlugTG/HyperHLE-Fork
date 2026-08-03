/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
// Allow the crate to have a non-snake-case name (radekHLE).
#![allow(non_snake_case)]

fn main() -> Result<(), String> {
    touchHLE::main(std::env::args())
}
