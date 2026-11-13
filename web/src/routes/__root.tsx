import { Link, Outlet, createRootRoute } from "@tanstack/react-router";

export const Route = createRootRoute({ component: RootLayout });

function RootLayout() {
  return (
    <div className="flex min-h-dvh flex-col">
      <header className="border-b">
        <div className="mx-auto flex w-full max-w-3xl items-center px-6 py-4">
          <Link to="/" className="font-semibold">
            Upland
          </Link>
        </div>
      </header>
      <main className="mx-auto w-full max-w-3xl flex-1 px-6 py-12">
        <Outlet />
      </main>
    </div>
  );
}
