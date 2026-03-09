/* SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me> */
/* SPDX-License-Identifier: GPL-3.0-or-later */

#ifndef VAUCHI_CABI_H
#define VAUCHI_CABI_H

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

/**
 * Opaque handle to a workflow engine instance.
 */
typedef struct VauchiWorkflow VauchiWorkflow;

/**
 * Free a string allocated by vauchi-cabi.
 *
 * # Safety
 * `ptr` must be a pointer returned by a vauchi_* function, or null.
 */
void vauchi_string_free(char *ptr);

/**
 * Create a new workflow engine instance.
 *
 * Supported `workflow_type` values:
 * - `"onboarding"` — onboarding flow (no args)
 * - `"emergency_shred"` — emergency data wipe (no args)
 * - `"lock_screen"` — lock screen with 3 max attempts (no args)
 *
 * Returns null on unknown type or null input.
 *
 * # Safety
 * `workflow_type` must be a valid null-terminated C string, or null.
 */
struct VauchiWorkflow *vauchi_workflow_create(const char *workflow_type);

/**
 * Destroy a workflow engine instance.
 *
 * # Safety
 * `handle` must be a pointer returned by `vauchi_workflow_create`, or null.
 */
void vauchi_workflow_destroy(struct VauchiWorkflow *handle);

/**
 * Get the current screen as a JSON string.
 *
 * Returns null if the handle is null. Returns an error JSON object if
 * the internal lock is poisoned. The caller must free the returned
 * string with `vauchi_string_free`.
 *
 * # Safety
 * `handle` must be a valid workflow handle or null.
 */
char *vauchi_workflow_current_screen(struct VauchiWorkflow *handle);

/**
 * Handle a user action (JSON string) and return the result as JSON.
 *
 * Returns null if the handle is null. Returns an error JSON object if
 * the action JSON is null or invalid. The caller must free the returned
 * string with `vauchi_string_free`.
 *
 * # Safety
 * `handle` must be a valid workflow handle or null.
 * `action_json` must be a valid null-terminated C string, or null.
 */
char *vauchi_workflow_handle_action(struct VauchiWorkflow *handle, const char *action_json);

#endif  /* VAUCHI_CABI_H */
