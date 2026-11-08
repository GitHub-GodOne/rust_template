import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
export type RatholeProfile = { id:number; name:string; mode:string; config_toml:string; enabled:boolean; status:string; last_error?:string|null; running:boolean; created_at:string; updated_at:string };
export type SaveRatholeProfile = { name:string; mode:string; config_toml:string; enabled:boolean };
export async function fetchRatholeProfiles(){ const r=await apiClient.get<ApiResponse<RatholeProfile[]>>("/admin/rathole/profiles"); return unwrap(r.data); }
export async function createRatholeProfile(p:SaveRatholeProfile){ const r=await apiClient.post<ApiResponse<RatholeProfile>>("/admin/rathole/profiles",p); return unwrap(r.data); }
export async function updateRatholeProfile(id:number,p:SaveRatholeProfile){ const r=await apiClient.put<ApiResponse<RatholeProfile>>(`/admin/rathole/profiles/${id}`,p); return unwrap(r.data); }
export async function deleteRatholeProfile(id:number){ await apiClient.delete(`/admin/rathole/profiles/${id}`); }
export async function validateRatholeProfile(p:{mode:string;config_toml:string}){ const r=await apiClient.post<ApiResponse<{valid:boolean}>>("/admin/rathole/validate",p); return unwrap(r.data); }
export async function startRatholeProfile(id:number){ await apiClient.post(`/admin/rathole/profiles/${id}/start`); }
export async function stopRatholeProfile(id:number){ await apiClient.post(`/admin/rathole/profiles/${id}/stop`); }
export async function generateRatholeKeypair(curve="x25519"){ const r=await apiClient.post<ApiResponse<{private_key:string;public_key:string}>>("/admin/rathole/genkey",{curve}); return unwrap(r.data); }
