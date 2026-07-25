import { AuthForm } from "@/components/AuthForm";

export default function LoginPage() {
  return (
    <main className="flex flex-1 flex-col items-center justify-center gap-6 px-6">
      <h1 className="text-2xl font-semibold">Log in</h1>
      <AuthForm mode="login" />
    </main>
  );
}
