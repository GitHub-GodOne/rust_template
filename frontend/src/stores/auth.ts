import { create } from "zustand";
import type { AdminMenuItem, PermissionCode } from "../app/menu";

export type AuthUser = {
  pid: string;
  name: string;
  email?: string;
  isVerified?: boolean;
};

export type AuthRole = {
  id: number;
  name: string;
  code: string;
};

export type AuthTenant = {
  id: number;
  name: string;
  code: string;
  departments_enabled?: boolean;
};

export type AuthDepartment = {
  id: number;
  tenant_id: number;
  name: string;
  code: string;
};

export type AuthDataScope = {
  id: number;
  name: string;
  code: string;
};

type AuthState = {
  accessToken: string | null;
  refreshToken: string | null;
  user: AuthUser | null;
  roles: AuthRole[];
  permissions: PermissionCode[];
  menus: AdminMenuItem[];
  tenant: AuthTenant | null;
  departments: AuthDepartment[];
  currentDepartment: AuthDepartment | null;
  dataScopes: AuthDataScope[];
  effectiveDataScope: string;
  signIn: (payload: {
    token: string;
    refreshToken: string;
    user: AuthUser;
  }) => void;
  setTokens: (payload: { token: string; refreshToken: string }) => void;
  setSession: (payload: {
    user: AuthUser;
    roles: AuthRole[];
    permissions: PermissionCode[];
    menus: AdminMenuItem[];
    tenant: AuthTenant | null;
    departments: AuthDepartment[];
    currentDepartment: AuthDepartment | null;
    dataScopes: AuthDataScope[];
    effectiveDataScope: string;
  }) => void;
  signOut: () => void;
  hasPermission: (permission?: PermissionCode) => boolean;
};

const tokenKey = "gpt-images-admin-token";
const refreshTokenKey = "gpt-images-admin-refresh-token";
const userKey = "gpt-images-admin-user";

function readStoredUser() {
  const value = localStorage.getItem(userKey);
  if (!value) {
    return null;
  }

  try {
    return JSON.parse(value) as AuthUser;
  } catch {
    localStorage.removeItem(userKey);
    return null;
  }
}

export const useAuthStore = create<AuthState>((set, get) => ({
  accessToken: localStorage.getItem(tokenKey),
  refreshToken: localStorage.getItem(refreshTokenKey),
  user: readStoredUser(),
  roles: [],
  permissions: [],
  menus: [],
  tenant: null,
  departments: [],
  currentDepartment: null,
  dataScopes: [],
  effectiveDataScope: "none",
  signIn: ({ token, refreshToken, user }) => {
    localStorage.setItem(tokenKey, token);
    localStorage.setItem(refreshTokenKey, refreshToken);
    localStorage.setItem(userKey, JSON.stringify(user));
    set({ accessToken: token, refreshToken, user });
  },
  setTokens: ({ token, refreshToken }) => {
    localStorage.setItem(tokenKey, token);
    localStorage.setItem(refreshTokenKey, refreshToken);
    set({ accessToken: token, refreshToken });
  },
  setSession: ({
    user,
    roles,
    permissions,
    menus,
    tenant,
    departments,
    currentDepartment,
    dataScopes,
    effectiveDataScope,
  }) => {
    localStorage.setItem(userKey, JSON.stringify(user));
    set({
      user,
      roles,
      permissions,
      menus,
      tenant,
      departments,
      currentDepartment,
      dataScopes,
      effectiveDataScope,
    });
  },
  signOut: () => {
    localStorage.removeItem(tokenKey);
    localStorage.removeItem(refreshTokenKey);
    localStorage.removeItem(userKey);
    set({
      accessToken: null,
      refreshToken: null,
      user: null,
      roles: [],
      permissions: [],
      menus: [],
      tenant: null,
      departments: [],
      currentDepartment: null,
      dataScopes: [],
      effectiveDataScope: "none",
    });
  },
  hasPermission: (permission) => {
    if (!permission) {
      return true;
    }

    const state = get();
    if (state.roles.some((role) => role.code === "super_admin")) {
      return true;
    }

    return state.permissions.includes(permission);
  },
}));
