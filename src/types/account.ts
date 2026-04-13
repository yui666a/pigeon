export type AccountProvider = "google" | "other";

export type OAuthStatus = "idle" | "waiting" | "exchanging" | "error";

export interface Account {
  id: string;
  name: string;
  email: string;
  imap_host: string;
  imap_port: number;
  smtp_host: string;
  smtp_port: number;
  auth_type: "plain" | "oauth2";
  provider: AccountProvider;
  needs_reauth: boolean;
  created_at: string;
}

export interface CreateAccountRequest {
  name: string;
  email: string;
  imap_host: string;
  imap_port: number;
  smtp_host: string;
  smtp_port: number;
  auth_type: "plain" | "oauth2";
  password: string;
}
