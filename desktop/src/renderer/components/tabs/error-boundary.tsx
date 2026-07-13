import { Component, type ErrorInfo, type ReactNode } from "react";

type TabErrorBoundaryProps = {
  children: ReactNode;
};

type TabErrorBoundaryState = {
  error: Error | null;
};

export class TabErrorBoundary extends Component<TabErrorBoundaryProps, TabErrorBoundaryState> {
  override state: TabErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): TabErrorBoundaryState {
    return { error };
  }

  override componentDidCatch(error: Error, errorInfo: ErrorInfo): void {
    console.error("Tab crashed", error, errorInfo);
  }

  override render() {
    if (this.state.error) {
      return (
        <div className="grid h-full min-h-0 place-items-center overflow-hidden p-5 text-center">
          <div className="max-w-sm">
            <h2 className="text-sm font-medium">This tab crashed</h2>
            <p className="mt-2 text-xs text-muted-foreground">{this.state.error.message}</p>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}
