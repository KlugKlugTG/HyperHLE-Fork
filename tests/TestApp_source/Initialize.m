/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

// Exercises the Objective-C runtime's `+initialize` semantics, which Apple
// documents at:
//   https://developer.apple.com/documentation/objectivec/nsobject/1418639-initialize
//
// Specifically:
//   * `+initialize` is invoked exactly once per class, lazily, before the
//     first message is sent to the class or any of its instances.
//   * It is inherited by subclasses, so messaging a subclass that doesn't
//     itself implement `+initialize` calls the parent's implementation a
//     second time, with `self` set to the subclass.
//   * Sending an unrelated class message such as `+class` is enough to
//     trigger initialization on demand.

#include "system_headers.h"

static int InitializeRoot_initialize_calls = 0;
static int InitializeChild_initialize_calls = 0;

@interface InitializeRoot : NSObject
@end

@implementation InitializeRoot
+ (void)initialize {
  InitializeRoot_initialize_calls++;
}
@end

@interface InitializeChild : InitializeRoot
@end

@implementation InitializeChild
// Intentionally no +initialize so we exercise inherited dispatch.
@end

@interface InitializeSibling : InitializeRoot
@end

@implementation InitializeSibling
+ (void)initialize {
  // Different class: this should be counted by InitializeRoot's hook (via
  // inheritance) *and* run its own override. The runtime first sends the
  // parent's +initialize with `self = InitializeSibling`, then ours.
  InitializeRoot_initialize_calls++;
  InitializeChild_initialize_calls++;
}
@end

int test_Initialize(void) {
  // Sanity: nothing should have run yet.
  if (InitializeRoot_initialize_calls != 0 ||
      InitializeChild_initialize_calls != 0) {
    return -1;
  }

  // Touch the root class. +initialize must fire exactly once.
  (void)[InitializeRoot class];
  if (InitializeRoot_initialize_calls != 1) {
    return -2;
  }

  // Touching it again must NOT re-run +initialize.
  (void)[InitializeRoot class];
  (void)[[InitializeRoot new] release];
  if (InitializeRoot_initialize_calls != 1) {
    return -3;
  }

  // Touching the subclass that does NOT override +initialize must call the
  // parent's implementation once more (with self = InitializeChild).
  (void)[InitializeChild class];
  if (InitializeRoot_initialize_calls != 2) {
    return -4;
  }
  (void)[InitializeChild class];
  if (InitializeRoot_initialize_calls != 2) {
    return -5;
  }

  // Touching a sibling that DOES override +initialize: the runtime calls the
  // parent's hook first (with self = InitializeSibling), then runs the
  // sibling's own override.
  (void)[InitializeSibling class];
  if (InitializeRoot_initialize_calls != 3 ||
      InitializeChild_initialize_calls != 1) {
    return -6;
  }
  (void)[InitializeSibling class];
  if (InitializeRoot_initialize_calls != 3 ||
      InitializeChild_initialize_calls != 1) {
    return -7;
  }

  return 0;
}
