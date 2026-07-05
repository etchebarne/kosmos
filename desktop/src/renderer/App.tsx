import { Button } from "@/components/ui/button";

export function App() {
  return (
    <main className="grid min-h-screen place-items-center bg-background text-foreground">
      <section className="grid gap-4 text-center">
        <h1 className="text-5xl font-semibold tracking-tight">Hello world</h1>
        <Button type="button">shadcn/ui ready</Button>
      </section>
    </main>
  );
}
