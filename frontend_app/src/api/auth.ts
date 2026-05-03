import type { Argon2Params, LoginResponse, SaltResponse } from '../types';
import { api } from './client';

export interface KeyBundleResponse {
  classical_public_key: string;
  pq_public_key: string;
  classical_priv_encrypted: string;
  pq_priv_encrypted: string;
  argon2_salt: string;
  argon2_params: Argon2Params;
  key_version: number;
}

export interface RotateCredentialsBody {
  current_auth_token: string;
  new_auth_token: string;
  new_argon2_salt: string;
  new_argon2_params: Argon2Params;
  new_classical_priv_encrypted: string;
  new_pq_priv_encrypted: string;
  new_key_version: number;
  updated_vaults: Array<{
    id: string;
    title_encrypted: string;
    vault_note_encrypted: string | null;
  }>;
}

export const authApi = {
  getSalt: (username: string) =>
    api.get<SaltResponse>(`/auth/salt?username=${encodeURIComponent(username)}`),

  register: (body: {
    username: string;
    auth_token: string;
    argon2_salt: string;
    argon2_params: { m_cost: number; t_cost: number; p_cost: number };
    classical_public_key: string;
    pq_public_key: string;
    classical_priv_encrypted: string;
    pq_priv_encrypted: string;
  }) => api.post<{ message: string }>('/auth/register', body),

  login: (username: string, auth_token: string) =>
    api.post<LoginResponse>('/auth/login', { username, auth_token }),

  deleteProfile: () =>
    api.delete<{ message: string; deleted_vaults: number }>('/auth/profile'),

  getKeyBundle: () =>
    api.get<KeyBundleResponse>('/auth/keys'),

  rotateCredentials: (body: RotateCredentialsBody) =>
    api.post<{ ok: boolean; access_token: string; refresh_token: string }>(
      '/auth/rotate-credentials',
      body,
    ),
};
