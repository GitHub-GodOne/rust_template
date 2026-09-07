import {
  CloseOutlined,
  EnterOutlined,
  SearchOutlined,
} from "@ant-design/icons";
import { Button, Empty, Input, Modal, Typography } from "antd";
import type { InputRef } from "antd";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import {
  type AdminMenuItem,
  defaultAdminMenus,
  mergeMenuItems,
} from "../app/menu";
import { useAuthStore } from "../stores/auth";
import { renderMenuIcon } from "./icons";

type MenuDestination = {
  key: string;
  label: string;
  path: string;
  icon?: string;
  parents: string[];
  searchText: string;
};

function normalizeSearchText(value: string) {
  return value.normalize("NFKC").toLocaleLowerCase().trim();
}

function collectMenuDestinations(
  menus: AdminMenuItem[],
  hasPermission: (permission?: string) => boolean,
  parents: string[] = [],
): MenuDestination[] {
  return menus.flatMap((item) => {
    if (!hasPermission(item.permission ?? item.permission_code)) {
      return [];
    }

    const nextParents = [...parents, item.label];
    const destination = item.path
      ? [
          {
            key: item.path,
            label: item.label,
            path: item.path,
            icon: item.icon,
            parents,
            searchText: normalizeSearchText(
              [item.label, item.title, item.path, ...nextParents]
                .filter(Boolean)
                .join(" "),
            ),
          },
        ]
      : [];

    return [
      ...destination,
      ...collectMenuDestinations(
        item.children ?? [],
        hasPermission,
        nextParents,
      ),
    ];
  });
}

export function MenuCommandPalette({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const menus = useAuthStore((state) => state.menus);
  const roles = useAuthStore((state) => state.roles);
  const permissions = useAuthStore((state) => state.permissions);
  const navigate = useNavigate();
  const location = useLocation();
  const inputRef = useRef<InputRef>(null);
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const visibleMenus = useMemo(
    () =>
      menus.length > 0
        ? mergeMenuItems(menus, defaultAdminMenus)
        : defaultAdminMenus,
    [menus],
  );
  const hasPermission = useCallback(
    (permission?: string) =>
      !permission ||
      roles.some((role) => role.code === "super_admin") ||
      permissions.includes(permission),
    [permissions, roles],
  );
  const destinations = useMemo(
    () => collectMenuDestinations(visibleMenus, hasPermission),
    [hasPermission, visibleMenus],
  );
  const normalizedQuery = normalizeSearchText(query);
  const filteredDestinations = useMemo(
    () =>
      normalizedQuery
        ? destinations.filter((item) =>
            item.searchText.includes(normalizedQuery),
          )
        : destinations,
    [destinations, normalizedQuery],
  );

  useEffect(() => {
    const handleShortcut = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        onOpenChange(true);
      }
    };

    window.addEventListener("keydown", handleShortcut);
    return () => window.removeEventListener("keydown", handleShortcut);
  }, [onOpenChange]);

  useEffect(() => {
    if (!open) {
      return;
    }

    setQuery("");
    setActiveIndex(
      Math.max(
        0,
        destinations.findIndex((item) => item.path === location.pathname),
      ),
    );
    const frame = window.requestAnimationFrame(() => inputRef.current?.focus());
    return () => window.cancelAnimationFrame(frame);
  }, [destinations, location.pathname, open]);

  const scrollActiveOptionIntoView = useCallback(
    (node: HTMLButtonElement | null) =>
      node?.scrollIntoView({ block: "nearest" }),
    [],
  );

  const close = () => onOpenChange(false);
  const openDestination = (destination?: MenuDestination) => {
    if (!destination) {
      return;
    }
    navigate(destination.path);
    close();
  };

  return (
    <Modal
      open={open}
      onCancel={close}
      footer={null}
      closable={false}
      width={640}
      className="menu-command-palette"
    >
      <div className="menu-command-search-row">
        <SearchOutlined aria-hidden />
        <Input
          ref={inputRef}
          value={query}
          variant="borderless"
          placeholder="搜索菜单或功能"
          aria-label="搜索菜单"
          aria-controls="menu-command-results"
          aria-expanded={open}
          role="combobox"
          onChange={(event) => {
            setQuery(event.target.value);
            setActiveIndex(0);
          }}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown") {
              event.preventDefault();
              setActiveIndex((current) =>
                filteredDestinations.length
                  ? (current + 1) % filteredDestinations.length
                  : 0,
              );
            } else if (event.key === "ArrowUp") {
              event.preventDefault();
              setActiveIndex((current) =>
                filteredDestinations.length
                  ? (current - 1 + filteredDestinations.length) %
                    filteredDestinations.length
                  : 0,
              );
            } else if (
              event.key === "Enter" &&
              !event.nativeEvent.isComposing
            ) {
              event.preventDefault();
              openDestination(filteredDestinations[activeIndex]);
            } else if (event.key === "Escape") {
              event.preventDefault();
              close();
            }
          }}
        />
        <Button
          type="text"
          className="menu-command-close"
          aria-label="关闭菜单搜索"
          icon={<CloseOutlined />}
          onClick={close}
        />
      </div>

      <div className="menu-command-section-head">
        <Typography.Text type="secondary">菜单</Typography.Text>
        <Typography.Text type="secondary">
          {filteredDestinations.length} 项
        </Typography.Text>
      </div>

      <div id="menu-command-results" className="menu-command-results">
        {filteredDestinations.length ? (
          filteredDestinations.map((destination, index) => {
            const active = index === activeIndex;
            return (
              <button
                key={destination.key}
                ref={active ? scrollActiveOptionIntoView : undefined}
                type="button"
                className={`menu-command-option${active ? " is-active" : ""}`}
                onMouseEnter={() => setActiveIndex(index)}
                onClick={() => openDestination(destination)}
              >
                <span className="menu-command-option-icon">
                  {renderMenuIcon(destination.icon)}
                </span>
                <span className="menu-command-option-copy">
                  <strong>{destination.label}</strong>
                  <span>
                    {destination.parents.length
                      ? destination.parents.join(" / ")
                      : destination.path}
                  </span>
                </span>
                <code className="menu-command-option-path">
                  {destination.path}
                </code>
                {active ? (
                  <EnterOutlined className="menu-command-enter" aria-hidden />
                ) : null}
              </button>
            );
          })
        ) : (
          <Empty
            className="menu-command-empty"
            image={Empty.PRESENTED_IMAGE_SIMPLE}
            description="没有匹配的菜单"
          />
        )}
      </div>
    </Modal>
  );
}
