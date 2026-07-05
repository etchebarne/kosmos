import { Button } from "@/renderer/components/ui/button";
import { Header } from "@/renderer/components/internal/header";

export function App() {
  return (
    <main className="flex h-full flex-col gap-2 overflow-hidden bg-muted text-foreground">
      <Header />

      <section className="grid min-h-0 flex-1 place-items-center gap-4 overflow-hidden rounded-2xl border bg-background text-center shadow-sm">
        <h1 className="text-5xl font-semibold tracking-tight">Hello world</h1>
        <Button type="button">shadcn/ui ready</Button>
      </section>
    </main>
  );
}
