import { useNavigate, useSearch } from "@tanstack/react-router";
import { useEffect, useState, type FormEvent } from "react";
import { useLogin, useMe } from "../api/queries";
import { ErrorMessage } from "../components/ErrorMessage";

export function LoginPage() {
  const { redirect } = useSearch({ from: "/login" });
  const navigate = useNavigate();
  const { data: me } = useMe();
  const login = useLogin();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");

  // Logged in (now or already): go on, once.
  const loggedIn = Boolean(me);
  useEffect(() => {
    if (loggedIn) void navigate({ to: redirect ?? "/", replace: true });
  }, [loggedIn, redirect, navigate]);

  if (me) return <p className="loading">Opening check_stock…</p>;

  const submit = (e: FormEvent) => {
    e.preventDefault();
    login.mutate({ username, password }, { onError: () => setPassword("") });
  };

  return (
    <main className="login">
      <form className="login__card" onSubmit={submit}>
        <h1>check_stock</h1>
        <label className="field">
          <span>User name</span>
          <input
            name="username"
            autoComplete="username"
            autoCapitalize="none"
            spellCheck={false}
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            required
          />
        </label>
        <label className="field">
          <span>Password</span>
          <input
            name="password"
            type="password"
            autoComplete="current-password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            required
          />
        </label>
        <ErrorMessage error={login.error} />
        <button type="submit" className="button button--primary" disabled={login.isPending}>
          {login.isPending ? "Logging in…" : "Log in"}
        </button>
      </form>
    </main>
  );
}
