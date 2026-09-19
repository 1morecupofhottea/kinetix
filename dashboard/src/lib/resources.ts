// Typed resource functions over the Kinetix admin API. Each returns the
// dashboard's camelCase view model (see mappers.ts).

import { api } from './api';
import {
  mapAccount,
  mapAlias,
  mapAudit,
  mapRoute,
  mapKey,
  mapMetrics,
  mapModel,
  mapProvider,
  mapRequest,
  mapLiveRequest,
} from './mappers';
import { Account, AuditLog, Route, ModelAlias, ModelConfig, Provider, ProxyMetrics, RequestLog, VirtualKey, LiveRequest } from '../types';

export interface CreateKeyInput {
  name: string;
  owner: string;
  tag: string;
  allowed_models: string[];
  rpm_limit?: number | null;
  tpm_limit?: number | null;
  daily_budget?: number | null;
  monthly_budget?: number | null;
}

export interface DiscoveredModel {
  id: string;
  display_name?: string | null;
  context_window?: number | null;
  max_output_tokens?: number | null;
  already_imported: boolean;
}

export interface TestResult {
  ok: boolean;
  status: number;
  latency_ms?: number;
  error?: string;
  response_preview?: string;
}

export const Kinetix = {
  // --- session -------------------------------------------------------------
  me: () => api.get<{ authenticated: boolean; user: string }>('/admin/api/me'),
  login: (password: string) => api.post<{ ok: boolean; user: string }>('/admin/api/login', { password }),
  logout: () => api.post<{ ok: boolean }>('/admin/api/logout'),

  // --- overview ------------------------------------------------------------
  async overview(): Promise<ProxyMetrics> {
    return mapMetrics(await api.get('/admin/api/overview'));
  },

  // --- virtual keys --------------------------------------------------------
  async keys(): Promise<VirtualKey[]> {
    const r = await api.get<{ keys: any[] }>('/admin/api/keys');
    return r.keys.map(mapKey);
  },
  async createKey(body: CreateKeyInput): Promise<{ key: VirtualKey; fullKey: string }> {
    const r = await api.post<{ key: any; full_key: string }>('/admin/api/keys', body);
    return { key: mapKey(r.key), fullKey: r.full_key };
  },
  updateKey: (id: string, body: Record<string, unknown>) => api.put(`/admin/api/keys/${id}`, body),
  deleteKey: (id: string) => api.del(`/admin/api/keys/${id}`),

  // --- providers -----------------------------------------------------------
  async providers(): Promise<Provider[]> {
    const r = await api.get<{ providers: any[] }>('/admin/api/providers');
    return r.providers.map(mapProvider);
  },
  createProvider: (body: Record<string, unknown>) => api.post('/admin/api/providers', body),
  updateProvider: (id: string, body: Record<string, unknown>) => api.put(`/admin/api/providers/${id}`, body),
  deleteProvider: (id: string) => api.del(`/admin/api/providers/${id}`),
  validateProvider: (body: Record<string, unknown>) =>
    api.post<{ valid: boolean; problems: string[]; warnings: string[]; outbound_security: string }>(
      '/admin/api/validate/provider',
      body,
    ),
  async discover(providerId: string): Promise<DiscoveredModel[]> {
    const r = await api.post<{ models: DiscoveredModel[] }>(`/admin/api/providers/${providerId}/discover`);
    return r.models;
  },
  test: (providerId: string, model: string) =>
    api.post<TestResult>(`/admin/api/providers/${providerId}/test`, { model }),

  // --- models --------------------------------------------------------------
  async models(): Promise<ModelConfig[]> {
    const r = await api.get<{ models: any[] }>('/admin/api/models');
    return r.models.map(mapModel);
  },
  createModel: (providerId: string, body: Record<string, unknown>) =>
    api.post(`/admin/api/providers/${providerId}/models`, body),
  updateModel: (id: string, body: Record<string, unknown>) => api.put(`/admin/api/models/${id}`, body),
  deleteModel: (id: string) => api.del(`/admin/api/models/${id}`),

  // --- accounts ------------------------------------------------------------
  async validateModel(body: Record<string, unknown>) {
    return api.post<{ valid: boolean; problems: string[]; warnings: string[] }>(
      '/admin/api/validate/model',
      body,
    );
  },
  async accounts(): Promise<Account[]> {
    const r = await api.get<{ accounts: any[] }>('/admin/api/accounts');
    return r.accounts.map(mapAccount);
  },
  async validateAccount(body: Record<string, unknown>) {
    return api.post<{ valid: boolean; problems: string[] }>('/admin/api/validate/account', body);
  },
  createAccount: (body: Record<string, unknown>) => api.post('/admin/api/accounts', body),
  updateAccount: (id: string, body: Record<string, unknown>) => api.put(`/admin/api/accounts/${id}`, body),
  deleteAccount: (id: string) => api.del(`/admin/api/accounts/${id}`),
  resetAccount: (id: string) => api.post(`/admin/api/accounts/${id}/reset`),

  // --- routes --------------------------------------------------------------
  async routes(): Promise<Route[]> {
    const r = await api.get<{ routes: any[] }>('/admin/api/routes');
    return r.routes.map(mapRoute);
  },
  createRoute: (body: Record<string, unknown>) => api.post('/admin/api/routes', body),
  updateRoute: (id: string, body: Record<string, unknown>) => api.put(`/admin/api/routes/${id}`, body),
  deleteRoute: (id: string) => api.del(`/admin/api/routes/${id}`),
  dryRunRoute: (model: string, descriptor: Record<string, unknown>) =>
    api.post<any>('/admin/api/routes/dry-run', { model, ...descriptor }),

  // --- aliases -------------------------------------------------------------
  async aliases(): Promise<ModelAlias[]> {
    const r = await api.get<{ aliases: any[] }>('/admin/api/aliases');
    return r.aliases.map(mapAlias);
  },
  createAlias: (body: Record<string, unknown>) => api.post('/admin/api/aliases', body),
  deleteAlias: (id: string) => api.del(`/admin/api/aliases/${id}`),

  // --- usage / requests ----------------------------------------------------
  async requests(limit = 200): Promise<RequestLog[]> {
    const r = await api.get<{ usage: any[] }>(`/admin/api/usage?limit=${limit}`);
    return r.usage.map(mapRequest);
  },

  // Live in-flight view (FR-8.3): metadata-only snapshot of requests currently
  // being served plus a short finished tail.
  async liveRequests(): Promise<LiveRequest[]> {
    const r = await api.get<{ live: any[] }>('/admin/api/requests/live');
    return r.live.map(mapLiveRequest);
  },

  // --- audit ---------------------------------------------------------------
  async audit(limit = 200): Promise<AuditLog[]> {
    const r = await api.get<{ audit: any[] }>(`/admin/api/audit?limit=${limit}`);
    return r.audit.map(mapAudit);
  },
};
