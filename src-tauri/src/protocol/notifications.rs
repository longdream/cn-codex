/// Server notification event names and dispatcher.
/// Instead of a large typed enum (like codex uses internally), cn-codex receives
/// JSON-RPC notifications from the external app-server process as raw JSON.
/// We match on the method name string and forward the params to Tauri events.

/// Maps a JSON-RPC notification method name to a Tauri event name.
pub fn notification_event_name(method: &str) -> &str {
    match method {
        // Turn lifecycle
        "turn/started" => "turn-started",
        "turn/completed" => "turn-completed",
        "turn/diff/updated" => "turn-diff-updated",
        "turn/plan/updated" => "turn-plan-updated",

        // Item lifecycle
        "item/started" => "item-started",
        "item/completed" => "item-completed",

        // Streaming deltas
        "item/agentMessage/delta" => "agent-message-delta",
        "item/reasoning/textDelta" => "reasoning-text-delta",
        "item/reasoning/summaryTextDelta" => "reasoning-summary-delta",
        "item/reasoning/summaryPartAdded" => "reasoning-summary-part-added",
        "item/plan/delta" => "plan-delta",

        // Command execution
        "command/exec/outputDelta" => "command-output-delta",
        "item/commandExecution/outputDelta" => "command-output-delta",

        // File changes
        "item/fileChange/outputDelta" => "file-change-output-delta",
        "item/fileChange/patchUpdated" => "file-change-patch-updated",

        // Hooks
        "hook/started" => "hook-started",
        "hook/completed" => "hook-completed",

        // Thread lifecycle
        "thread/started" => "thread-started",
        "thread/status/changed" => "thread-status-changed",
        "thread/name/updated" => "thread-name-updated",
        "thread/settingsUpdated" => "thread-settings-updated",
        "thread/goalUpdated" => "thread-goal-updated",
        "thread/goalCleared" => "thread-goal-cleared",
        "thread/tokenUsage/updated" => "thread-token-usage-updated",
        "thread/compacted" => "context-compacted",
        "thread/archived" => "thread-archived",
        "thread/unarchived" => "thread-unarchived",
        "thread/closed" => "thread-closed",

        // Guardian / auto-approval
        "item/autoApprovalReview/started" => "guardian-review-started",
        "item/autoApprovalReview/completed" => "guardian-review-completed",

        // Account
        "account/updated" => "account-updated",
        "account/rateLimits/updated" => "account-rate-limits-updated",
        "account/login/completed" => "account-login-completed",

        // Model
        "model/rerouted" => "model-rerouted",
        "model/verification" => "model-verification",

        // MCP
        "item/mcpToolCall/progress" => "mcp-tool-call-progress",
        "mcpServer/startupStatus/updated" => "mcp-server-status-updated",

        // Error/warning
        "error" => "server-error",
        "warning" => "server-warning",
        "configWarning" => "config-warning",

        // Skills
        "skills/changed" => "skills-changed",

        // Server request resolved
        "serverRequest/resolved" => "server-request-resolved",

        // Catch-all
        _ => "server-notification",
    }
}
