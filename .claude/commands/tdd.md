---
allowed-tools: Task, Read, Bash(cat)
description: Start TDD workflow for a feature - interprets your request and invokes tdd-architect
argument-hint: <feature description in plain english>
---

# 🏗️ TDD Feature Design

Start test-driven development for a new feature by describing what you want in plain English.

## Context Files

Current TDD progress: @TODO_TDD.md
Project structure: @SRC_FILE_MAP.md

## User Request
**Feature requested**: $ARGUMENTS

## Your Task

Analyze the user's request and intelligently invoke the tdd-architect agent:

### 1. Parse the Request
Understand what the user wants to build from their natural language description.

### 2. Check Current State
Review TODO_TDD.md to determine:
- Is this a new feature or continuation of existing work?
- What tasks are already completed?
- What's the next logical step?

### 3. Identify Integration Points
From SRC_FILE_MAP.md, identify:
- Which modules will this feature affect?
- What existing types/traits should be used?
- Where should new code be placed?

### 4. Create Enhanced Prompt
Rephrase the user's request into a technical prompt for tdd-architect that includes:
- Clear feature requirements
- Performance targets (if mentioned or inferred)
- Integration points with existing modules
- Current progress from TODO_TDD.md
- Specific next steps to take

### 5. Invoke tdd-architect
Use the Task tool to call the tdd-architect agent with your enhanced prompt.

## Example Transformations

**User says**: "add vector search"
**You create**: "Design test-driven API for vector search feature. Requirements: integrate with existing Symbol and DocumentIndex types from storage module. Target sub-10ms query latency for 1M vectors. Start with basic similarity search API in tests/poc_vector_search_test.rs."

**User says**: "make it handle errors better"  
**You create**: "Continue TDD for error handling in [feature from TODO_TDD.md]. Current state: happy path tests complete. Design comprehensive error cases using thiserror types, focusing on: invalid input, resource exhaustion, and partial failures."

**User says**: "finish the clustering"
**You create**: "Resume TDD implementation of clustering feature. Completed tasks from TODO_TDD.md: [list]. Next: design tests for cluster assignment and persistence. Use existing ClusterId type from POC tests."

## Important Notes

- If TODO_TDD.md doesn't exist, that's fine - treat as new feature
- Always provide specific technical details in the rephrased prompt
- Include performance/scale requirements even if user doesn't mention them
- Reference specific files and types from SRC_FILE_MAP.md
- Make the prompt actionable and specific for tdd-architect

Remember: You're the bridge between the user's idea and a well-structured TDD process. Transform vague requests into concrete, testable requirements.