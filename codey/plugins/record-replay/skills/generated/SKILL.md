---
name: 
description: "百度搜索关键词; 打开百度首页，在搜索框中输入关键词，然后点击\"百度一下\"按钮执行搜索，获取搜索结果页面。"
---

# 打开百度首页，在搜索框中输入关键词，然后点击"百度一下"按钮执行搜索，获取搜索结果页面。

## Objective

打开百度首页，在搜索框中输入关键词，然后点击"百度一下"按钮执行搜索，获取搜索结果页面。

This skill was generated from a recorded user demonstration. Execute it by understanding the goal and dynamically calling the appropriate tools — do NOT treat the steps below as a rigid script.

## When to Trigger

Activate this skill when the user's request matches any of these intents:
- "百度搜索关键词"
- "打开百度首页，在搜索框中输入关键词，然后点击"百度一下"按钮执行搜索，获取搜索结果页面。"

## Variables

No variables — this workflow uses fixed values from the recording.

Before executing, resolve all variables. If a variable cannot be determined from context, ask the user.

## Execution Strategy

This is a goal-driven workflow. The steps below describe the INTENDED sequence, but you should:
1. Check the current state before each step
2. Skip steps that are already satisfied
3. Adapt locators if the UI has changed
4. Verify each step's outcome before proceeding

### step_1: Type into 搜索

**Tool**: `browser_run`

**What to do**: Type `天气` into the field `#kw`

**Page context**: https://www.baidu.com

**Verify**: Verify input field contains the expected value

**If it fails**: Try alternative locator: [name='wd'] → Find by ARIA: role="textbox" name="搜索" → Take screenshot and ask agent to visually locate the element

---

### step_2: Click on 百度一下

**Tool**: `browser_run`

**What to do**: Click on the element identified by `#su`

**Page context**: https://www.baidu.com

**Verify**: Verify click had expected effect (element state changed)

**If it fails**: Try alternative locator: [value='百度一下'] → Find by visible text: "百度一下" → Find by ARIA: role="button" name="百度一下" → Take screenshot and ask agent to visually locate the element

**Requires**: step_1 to complete first

## Verification & Success Criteria

After completing all steps, verify the overall goal was achieved:
- All steps completed without unrecoverable errors
- Final verification passed: Verify click had expected effect (element state changed)
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
