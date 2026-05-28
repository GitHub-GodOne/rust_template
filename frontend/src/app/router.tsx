import { Spin } from "antd";
import type { ReactNode } from "react";
import { useEffect, useState } from "react";
import { Navigate, Route, Routes, useLocation } from "react-router-dom";
import { current } from "../api/auth";
import { AdminLayout } from "../layouts/AdminLayout";
import { LoginPage } from "../pages/auth/LoginPage";
import { DashboardPage } from "../pages/dashboard/DashboardPage";
import { BackupsPage } from "../pages/system/backups/BackupsPage";
import { EmailTemplatesPage } from "../pages/system/email-templates/EmailTemplatesPage";
import { LogsPage } from "../pages/system/logs/LogsPage";
import { MenusPage } from "../pages/system/menus/MenusPage";
import { MonitoringPage } from "../pages/system/monitoring/MonitoringPage";
import { NotificationsPage } from "../pages/system/notifications/NotificationsPage";
import { PermissionsPage } from "../pages/system/permissions/PermissionsPage";
import { RolesPage } from "../pages/system/roles/RolesPage";
import { ScheduledTasksPage } from "../pages/system/scheduled-tasks/ScheduledTasksPage";
import { SettingsPage } from "../pages/system/settings/SettingsPage";
import { TenantsPage } from "../pages/system/tenants/TenantsPage";
import { UploadsPage } from "../pages/system/uploads/UploadsPage";
import { UsersPage } from "../pages/system/users/UsersPage";
import { useAuthStore } from "../stores/auth";

function RequireAuth({ children }: { children: ReactNode }) {
  const token = useAuthStore((state) => state.accessToken);
  const signOut = useAuthStore((state) => state.signOut);
  const setSession = useAuthStore((state) => state.setSession);
  const location = useLocation();
  const [hydrating, setHydrating] = useState(Boolean(token));

  useEffect(() => {
    if (!token) {
      setHydrating(false);
      return;
    }

    let mounted = true;
    setHydrating(true);
    current()
      .then((session) => {
        if (!mounted) {
          return;
        }
        setSession({
          user: {
            pid: session.pid,
            name: session.name,
            email: session.email,
          },
          roles: session.roles,
          permissions: session.permissions,
          menus: session.menus,
          tenant: session.tenant ?? null,
          dataScopes: session.data_scopes,
          effectiveDataScope: session.effective_data_scope,
        });
      })
      .catch(() => {
        if (mounted) {
          signOut();
        }
      })
      .finally(() => {
        if (mounted) {
          setHydrating(false);
        }
      });

    return () => {
      mounted = false;
    };
  }, [token, setSession, signOut]);

  if (!token) {
    return <Navigate replace to="/login" state={{ from: location }} />;
  }

  if (hydrating) {
    return (
      <div className="route-loading">
        <Spin />
      </div>
    );
  }

  return children;
}

function RequirePermission({
  permission,
  children,
}: {
  permission: string;
  children: ReactNode;
}) {
  const hasPermission = useAuthStore((state) => state.hasPermission);

  if (!hasPermission(permission)) {
    return <Navigate replace to="/admin/dashboard" />;
  }

  return children;
}

export function AppRouter() {
  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      <Route
        path="/admin"
        element={
          <RequireAuth>
            <AdminLayout />
          </RequireAuth>
        }
      >
        <Route index element={<Navigate replace to="/admin/dashboard" />} />
        <Route
          path="dashboard"
          element={
            <RequirePermission permission="dashboard:view">
              <DashboardPage />
            </RequirePermission>
          }
        />
        <Route
          path="system/users"
          element={
            <RequirePermission permission="system:user:list">
              <UsersPage />
            </RequirePermission>
          }
        />
        <Route
          path="system/roles"
          element={
            <RequirePermission permission="system:role:list">
              <RolesPage />
            </RequirePermission>
          }
        />
        <Route
          path="system/tenants"
          element={
            <RequirePermission permission="system:tenant:list">
              <TenantsPage />
            </RequirePermission>
          }
        />
        <Route
          path="system/menus"
          element={
            <RequirePermission permission="system:menu:list">
              <MenusPage />
            </RequirePermission>
          }
        />
        <Route
          path="system/permissions"
          element={
            <RequirePermission permission="system:permission:list">
              <PermissionsPage />
            </RequirePermission>
          }
        />
        <Route
          path="system/logs"
          element={
            <RequirePermission permission="system:log:list">
              <LogsPage />
            </RequirePermission>
          }
        />
        <Route
          path="system/uploads"
          element={
            <RequirePermission permission="system:upload:list">
              <UploadsPage />
            </RequirePermission>
          }
        />
        <Route
          path="system/settings"
          element={
            <RequirePermission permission="system:setting:list">
              <SettingsPage />
            </RequirePermission>
          }
        />
        <Route
          path="system/notifications"
          element={
            <RequirePermission permission="system:notification:list">
              <NotificationsPage />
            </RequirePermission>
          }
        />
        <Route
          path="system/scheduled-tasks"
          element={
            <RequirePermission permission="system:scheduled_task:list">
              <ScheduledTasksPage />
            </RequirePermission>
          }
        />
        <Route
          path="system/backups"
          element={
            <RequirePermission permission="system:backup:list">
              <BackupsPage />
            </RequirePermission>
          }
        />
        <Route
          path="system/monitoring"
          element={
            <RequirePermission permission="system:monitor:view">
              <MonitoringPage />
            </RequirePermission>
          }
        />
        <Route
          path="system/email-templates"
          element={
            <RequirePermission permission="system:email_template:list">
              <EmailTemplatesPage />
            </RequirePermission>
          }
        />
      </Route>
      <Route path="*" element={<Navigate replace to="/admin/dashboard" />} />
    </Routes>
  );
}
