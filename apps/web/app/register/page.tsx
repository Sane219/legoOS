import { AuthForm } from "@/components/AuthForm";

export default function RegisterPage() {
  return (
    <main className="flex flex-1 flex-col items-center justify-center gap-6 px-6">
      <h1 className="text-2xl font-semibold">Create your account</h1>
      <AuthForm mode="register" />
    </main>
  );
}
