---
name: login-to-application
description: "login to application; log into browser at https://example"
---

# Log into Browser at https://example.com/login using user credentials.

## Objective

Log into Browser at https://example.com/login using user credentials.

This skill was generated from a recorded user demonstration. Execute it by understanding the goal and dynamically calling the appropriate tools — do NOT treat the steps below as a rigid script.

## When to Trigger

Activate this skill when the user's request matches any of these intents:
- "login to application"
- "log into browser at https://example"

## Variables

- `{{email_0}}` — **email**: email value for "email"
  - Default: `user@example.com`
- `{{input_1}}` — **string**: string value for "password"
  - Default: `secret123`

Before executing, resolve all variables. If a variable cannot be determined from context, ask the user.

## Execution Strategy

This is a goal-driven workflow. The steps below describe the INTENDED sequence, but you should:
1. Check the current state before each step
2. Skip steps that are already satisfied
3. Adapt locators if the UI has changed
4. Verify each step's outcome before proceeding

### step_1: Type into email

**Tool**: `browser_run`

**What to do**: Type `{{email_0}}` into the field `#email`

**Page context**: https://example.com/login

**Verify**: Verify input field contains the expected value

**If it fails**: Try alternative locator: [name='email'] → Find by ARIA: role="textbox" name="email" → Take screenshot and ask agent to visually locate the element

---

### step_2: Type into password

**Tool**: `browser_run`

**What to do**: Type `{{input_1}}` into the field `#password`

**Page context**: https://example.com/login

**Verify**: Verify input field contains the expected value

**If it fails**: Try alternative locator: [name='password'] → Find by ARIA: role="textbox" name="password" → Take screenshot and ask agent to visually locate the element

**Requires**: step_1 to complete first

---

### step_3: Click on Sign In

**Tool**: `browser_run`

**What to do**: Click on the element identified by `button[type='submit']`

**Page context**: https://example.com/login

**Verify**: Verify expected UI change: new element appears or state updates

**If it fails**: Try alternative locator: .login-btn → Find by visible text: "Sign In" → Find by ARIA: role="button" name="Sign In" → Take screenshot and ask agent to visually locate the element

**Requires**: step_2 to complete first

---

### step_4: Submit form

**Tool**: `browser_run`

**What to do**: Click on the element identified by `form.login-form`

**Page context**: https://example.com/login

**Verify**: Verify form submission succeeded: look for success message, redirect, or state change

**If it fails**: Try alternative locator: form → Find by ARIA: role="form" name="login-form" → Take screenshot and ask agent to visually locate the element

**Requires**: step_3 to complete first

## Verification & Success Criteria

After completing all steps, verify the overall goal was achieved:
- All steps completed without unrecoverable errors
- Final verification passed: Verify form submission succeeded: look for success message, redirect, or state change
- Take a final screenshot to confirm the expected end state

## Recovery Policy

- **Max retries per step**: 2
- **Primary fallback**: retry_with_alternative_locator
- **If locator fails**: Try alternative selectors, then find by visible text, then screenshot + visual search
- **If page state unexpected**: Take a screenshot, describe the current state, and decide whether to continue or abort
- **If all retries exhausted**: Report failure to the user with evidence (screenshot + description of what went wrong)

## Execution Notes for the Agent

1. **Adapt, don't replay**: The recorded workflow captured one path. The current UI may differ. Use the locator candidates and element descriptions to find the right targets.
2. **Verify before acting**: Before clicking or typing, confirm the target element exists and is in the expected state.
3. **Use screenshots strategically**: Take screenshots at key checkpoints to verify progress, especially after navigation and form submissions.
4. **Variable resolution**: Replace all `{{variable}}` placeholders with actual values before executing. Use defaults if the user doesn't provide overrides.
5. **Report progress**: Keep the user informed of major milestones (e.g., "Logged in successfully", "Form submitted").
