---
name: document-writer
description: Produce structured technical documents including specs, user manuals, API guides, architecture docs, proposals, and meeting notes. Outputs Markdown by default; can render to DOCX via the documents plugin when requested. Use when the user asks to "write a doc", "create a spec", "draft a manual", "write documentation", "create an API guide", "architecture document", "technical proposal", "meeting notes", "design doc", or any structured multi-section document that is not a report/analysis (use report-generation for those). Part of the Document Processing set.
---

# Document Writer Skill

Produce structured, professional technical documents. This skill handles the planning, outlining, drafting, and formatting of documents that follow established conventions for their type.

## When to use

- Technical specifications / design documents
- User manuals / how-to guides
- API documentation / reference guides
- Architecture decision records (ADRs)
- Proposals / RFCs
- Meeting notes / minutes
- Standard operating procedures (SOPs)
- Release notes / changelogs
- Onboarding guides

## When NOT to use

- Research reports, analyses, deep-dives → use `report-generation`
- E-commerce product descriptions → use `batch-production`
- Short conversational answers or single-file code edits
- Slide decks or presentations

## Inputs to gather (ask only what's missing)

- **Document type** (spec, manual, API guide, proposal, etc.)
- **Audience** (developers, end users, stakeholders, mixed)
- **Scope** — what system/feature/topic to document
- **Output format** preference: Markdown (default) or DOCX
- **Constraints** — existing templates, style guides, page limits

## Workflow

### 1. Determine document type and template

Each document type has a canonical structure. Select the appropriate one:

| Type | Key Sections |
|------|-------------|
| Technical Spec | Overview, Goals/Non-goals, Design, API, Testing, Rollout |
| User Manual | Getting Started, Installation, Usage, Troubleshooting, FAQ |
| API Guide | Authentication, Endpoints, Request/Response, Errors, Examples |
| ADR | Context, Decision, Consequences, Status |
| Proposal/RFC | Problem, Proposed Solution, Alternatives, Timeline, Risks |
| SOP | Purpose, Scope, Responsibilities, Procedure, Records |
| Release Notes | Version, Date, New Features, Bug Fixes, Breaking Changes, Migration |

### 2. Create outline

Before drafting, produce a structured outline with:
- All major sections identified
- Subsection breakdown for complex sections
- Placeholder notes for diagrams/tables needed
- Estimated length per section

Present the outline to the user for approval before proceeding.

### 3. Draft content

Write following these principles:

**Clarity over cleverness:**
- Use simple, direct language
- One idea per sentence
- Active voice preferred
- Define jargon on first use

**Structure for scanning:**
- Lead with the most important information
- Use bullet lists for 3+ related items
- Tables for comparative or tabular data
- Code blocks with language tags for all code

**Completeness:**
- Every section must be substantive (no "TBD" or "TODO" in final output)
- Include concrete examples for abstract concepts
- Cross-reference related sections

### 4. Add metadata

Every document starts with a metadata block:

```markdown
---
title: Document Title
type: spec | manual | api-guide | adr | proposal | sop | release-notes
author: CN-Codex AI Assistant
date: YYYY-MM-DD
version: 1.0
status: draft | review | approved | archived
audience: developers | end-users | stakeholders
---
```

### 5. Review and polish

Before delivering:
- Verify all cross-references resolve
- Check code examples are syntactically correct
- Ensure consistent terminology throughout
- Validate table formatting
- Confirm heading hierarchy is clean (no skipped levels)

## Output formats

### Markdown (default)
Save to `docs/<type>/<slug>.md`. Self-contained, version-control friendly.

### DOCX (when requested)
Use the **documents plugin** (`codey/plugins/documents`) for professional DOCX output:

1. Write the document content as Markdown first
2. Convert to DOCX using the documents plugin's `render_docx.py`:
   ```bash
   python codey/plugins/documents/skills/documents/render_docx.py output.docx
   ```
3. Apply design presets from `references/design_presets.md`:
   - `standard_business_brief` for formal specs and proposals
   - `compact_reference_guide` for manuals and SOPs
   - `narrative_proposal` for RFCs and longer-form proposals
4. Render and visually inspect page PNGs before delivery

### HTML Report (for presentation-quality output)
Use the **report-generation** skill when the document needs to be read in-browser with rich formatting.

## Document type templates

### Technical Specification

```markdown
---
title: [Feature] Technical Specification
type: spec
---

## Overview
*One paragraph summarizing what this spec covers and why.*

## Goals and Non-goals

### Goals
- [Measurable outcome 1]
- [Measurable outcome 2]

### Non-goals
- [Explicitly out of scope item]

## Background
Context the reader needs to understand the design.

## Detailed Design

### Architecture
[System diagram or description]

### API Surface
[Interface definitions]

### Data Model
[Schema or structure]

## Alternatives Considered
| Option | Pros | Cons | Verdict |
|--------|------|------|---------|

## Testing Strategy
How this will be validated.

## Rollout Plan
Phases, feature flags, monitoring.

## Open Questions
Items requiring further discussion.
```

### User Manual

```markdown
---
title: [Product] User Manual
type: manual
---

## Getting Started
What the product does and who it's for.

## Installation / Setup
Step-by-step setup instructions.

## Core Features
### [Feature A]
What it does, how to use it, with screenshots/examples.

### [Feature B]
...

## Advanced Usage
Power-user workflows and configuration.

## Troubleshooting
| Symptom | Cause | Fix |
|---------|-------|-----|

## FAQ
Common questions and answers.

## Appendix
Reference tables, shortcuts, glossary.
```

### API Documentation

```markdown
---
title: [Service] API Reference
type: api-guide
---

## Overview
Base URL, versioning scheme, rate limits.

## Authentication
How to obtain and use credentials.

## Endpoints

### `POST /resource`
**Description:** Create a new resource.

**Request:**
| Field | Type | Required | Description |
|-------|------|----------|-------------|

**Response:** `201 Created`

**Example:**
[Request/response pair]

**Errors:**
| Code | Meaning |
|------|---------|

## Error Handling
Global error format and common codes.

## SDKs and Examples
Links to client libraries and complete examples.
```

## Quality bar

- Every section is substantive — no stubs or placeholders in final output
- Consistent voice and terminology throughout
- All code examples are syntactically valid
- Tables are properly formatted and aligned
- Heading hierarchy is clean (h2 > h3 > h4, no skips)
- Document can stand alone without conversation context
- Metadata block is complete and accurate

## Common pitfalls

- **Wall of text** — break long paragraphs, add lists and tables
- **Inconsistent terminology** — pick one term and stick with it
- **Missing context** — don't assume reader has conversation history
- **Over-nesting** — h4 is the deepest useful level; flatten if deeper
- **Code without context** — always explain what the code does before showing it

## Integration with other skills

- For visual/designed output → `report-generation`
- For DOCX with design presets → documents plugin
- For architecture diagrams → use Mermaid in fenced blocks
- For API testing documentation → `api-testing` skill from qa-test-bot
