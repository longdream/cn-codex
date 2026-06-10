import React from "react";

interface Props {
  children: React.ReactNode;
}

interface State {
  hasError: boolean;
  error: Error | null;
}

export class ErrorBoundary extends React.Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    console.error("ErrorBoundary caught:", error, info.componentStack);
  }

  render() {
    if (this.state.hasError) {
      return (
        <div style={{
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          justifyContent: "center",
          height: "100vh",
          padding: "2rem",
          fontFamily: "system-ui, sans-serif",
          background: "#1a1a2e",
          color: "#e0e0e0",
        }}>
          <h2 style={{ marginBottom: "1rem", color: "#ff6b6b" }}>
            Application Error
          </h2>
          <p style={{ marginBottom: "1rem", opacity: 0.7, fontSize: "0.9rem", textAlign: "center", maxWidth: "500px" }}>
            An unexpected error occurred. Please restart the application.
          </p>
          <pre style={{
            padding: "1rem",
            background: "#0d0d1a",
            borderRadius: "8px",
            fontSize: "0.8rem",
            maxWidth: "600px",
            overflow: "auto",
            whiteSpace: "pre-wrap",
            wordBreak: "break-word",
          }}>
            {this.state.error?.message ?? "Unknown error"}
          </pre>
          <button
            onClick={() => this.setState({ hasError: false, error: null })}
            style={{
              marginTop: "1.5rem",
              padding: "0.6rem 1.5rem",
              background: "#4a90d9",
              color: "white",
              border: "none",
              borderRadius: "6px",
              cursor: "pointer",
              fontSize: "0.9rem",
            }}
          >
            Try Again
          </button>
        </div>
      );
    }

    return this.props.children;
  }
}
