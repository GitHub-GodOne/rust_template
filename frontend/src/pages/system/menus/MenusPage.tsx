import {
  DeleteOutlined,
  EditOutlined,
  MenuOutlined,
  PlusOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Form,
  Input,
  InputNumber,
  Modal,
  Select,
  Space,
  Switch,
  Tag,
  Tree,
  Typography,
  message,
} from "antd";
import type { TreeProps } from "antd";
import { useEffect, useMemo, useState } from "react";
import {
  type MenuRecord,
  type SaveMenuParams,
  createMenu,
  deleteMenu,
  fetchMenus,
  updateMenu,
} from "../../../api/admin/menus";
import { current } from "../../../api/auth";
import { CrudPage } from "../../../components/admin/CrudPage";
import { CrudToolbar } from "../../../components/admin/CrudToolbar";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { menuIconOptions, renderMenuIcon } from "../../../layouts/icons";
import { useAuthStore } from "../../../stores/auth";

type MenuTreeNode = Omit<MenuRecord, "children"> & {
  key: number;
  children: MenuTreeNode[];
};

function flattenMenus(menus: MenuRecord[]): MenuRecord[] {
  return menus.flatMap((menu) => [menu, ...flattenMenus(menu.children ?? [])]);
}

function toMenuTree(menus: MenuRecord[]): MenuTreeNode[] {
  return menus.map((menu) => ({
    ...menu,
    key: menu.id,
    children: toMenuTree(menu.children ?? []),
  }));
}

function collectMenuIds(menu: MenuTreeNode): Set<number> {
  return new Set([
    menu.id,
    ...menu.children.flatMap((child) => Array.from(collectMenuIds(child))),
  ]);
}

function removeMenuNode(
  menus: MenuTreeNode[],
  id: number,
): { menus: MenuTreeNode[]; removed?: MenuTreeNode } {
  let removed: MenuTreeNode | undefined;
  const next = menus.flatMap((menu) => {
    if (menu.id === id) {
      removed = menu;
      return [];
    }
    const childResult = removeMenuNode(menu.children, id);
    if (childResult.removed) {
      removed = childResult.removed;
      return [{ ...menu, children: childResult.menus }];
    }
    return [menu];
  });
  return { menus: next, removed };
}

function findMenuLocation(
  menus: MenuTreeNode[],
  id: number,
  parentId: number | null = null,
): { node: MenuTreeNode; parentId: number | null; index: number } | null {
  for (const [index, menu] of menus.entries()) {
    if (menu.id === id) {
      return { node: menu, parentId, index };
    }
    const child = findMenuLocation(menu.children, id, menu.id);
    if (child) {
      return child;
    }
  }
  return null;
}

function insertMenuNode(
  menus: MenuTreeNode[],
  parentId: number | null,
  index: number,
  node: MenuTreeNode,
): MenuTreeNode[] {
  if (parentId === null) {
    const next = [...menus];
    next.splice(index, 0, node);
    return next;
  }
  return menus.map((menu) =>
    menu.id === parentId
      ? {
          ...menu,
          children: insertMenuNode(menu.children, null, index, node),
        }
      : {
          ...menu,
          children: insertMenuNode(menu.children, parentId, index, node),
        },
  );
}

function menuSaveParams(
  menu: MenuTreeNode,
  parentId: number | null,
  sortOrder: number,
): SaveMenuParams {
  return {
    parent_id: parentId,
    title: menu.title,
    path: menu.path,
    icon: menu.icon,
    permission_code: menu.permission_code,
    sort_order: sortOrder,
    visible: menu.visible,
    enabled: menu.enabled,
  };
}

function flattenMenuTree(
  menus: MenuTreeNode[],
  parentId: number | null = null,
): Array<{ menu: MenuTreeNode; parentId: number | null; sortOrder: number }> {
  return menus.flatMap((menu, index) => [
    { menu, parentId, sortOrder: (index + 1) * 10 },
    ...flattenMenuTree(menu.children, menu.id),
  ]);
}

function iconSelectOptions(currentIcon?: string | null) {
  if (
    !currentIcon ||
    menuIconOptions.some((option) => option.value === currentIcon)
  ) {
    return menuIconOptions;
  }
  return [
    ...menuIconOptions,
    {
      value: currentIcon,
      label: currentIcon,
      icon: renderMenuIcon(currentIcon),
    },
  ];
}

export function MenusPage() {
  const [editing, setEditing] = useState<MenuRecord | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [menuTree, setMenuTree] = useState<MenuTreeNode[]>([]);
  const [form] = Form.useForm<SaveMenuParams>();
  const queryClient = useQueryClient();
  const setMenus = useAuthStore((state) => state.setMenus);
  const canReorder = useAuthStore((state) =>
    state.hasPermission("system:menu:update"),
  );

  const menusQuery = useQuery({
    queryKey: ["admin-menus"],
    queryFn: fetchMenus,
  });
  useEffect(() => {
    setMenuTree(toMenuTree(menusQuery.data ?? []));
  }, [menusQuery.data]);

  const refreshMenuViews = async () => {
    await queryClient.invalidateQueries({ queryKey: ["admin-menus"] });
    try {
      const session = await current();
      setMenus(session.menus);
    } catch {
      message.warning("菜单已保存，导航刷新失败，请刷新页面后查看");
    }
  };

  const flatMenus = useMemo(() => flattenMenus(menuTree), [menuTree]);
  const editingMenuIds = useMemo(() => {
    if (!editing) {
      return new Set<number>();
    }
    const node = findMenuLocation(menuTree, editing.id)?.node;
    return node ? collectMenuIds(node) : new Set([editing.id]);
  }, [editing, menuTree]);
  const saveMutation = useMutation({
    mutationFn: (values: SaveMenuParams) =>
      editing ? updateMenu(editing.id, values) : createMenu(values),
    onSuccess: async () => {
      message.success("菜单已保存");
      setFormOpen(false);
      setEditing(null);
      form.resetFields();
      await refreshMenuViews();
    },
  });
  const reorderMutation = useMutation({
    mutationFn: async (nextTree: MenuTreeNode[]) => {
      const original = new Map(
        flattenMenus(menusQuery.data ?? []).map((menu) => [menu.id, menu]),
      );
      const changed = flattenMenuTree(nextTree).filter(
        ({ menu, parentId, sortOrder }) => {
          const current = original.get(menu.id);
          return (
            current &&
            ((current.parent_id ?? null) !== parentId ||
              current.sort_order !== sortOrder)
          );
        },
      );
      await Promise.all(
        changed.map(({ menu, parentId, sortOrder }) =>
          updateMenu(menu.id, menuSaveParams(menu, parentId, sortOrder)),
        ),
      );
    },
    onMutate: (nextTree) => {
      const previousTree = menuTree;
      setMenuTree(nextTree);
      return { previousTree };
    },
    onSuccess: async () => {
      message.success("菜单顺序已更新");
      await refreshMenuViews();
    },
    onError: (error, _nextTree, context) => {
      if (context?.previousTree) {
        setMenuTree(context.previousTree);
      }
      message.error(error instanceof Error ? error.message : "菜单排序失败");
    },
  });
  const deleteMutation = useMutation({
    mutationFn: deleteMenu,
    onSuccess: async () => {
      message.success("菜单已删除");
      await refreshMenuViews();
    },
  });

  const openEditor = (record: MenuRecord) => {
    setEditing(record);
    form.setFieldsValue({
      parent_id: record.parent_id,
      title: record.title,
      path: record.path,
      icon: record.icon,
      permission_code: record.permission_code,
      sort_order: record.sort_order,
      visible: record.visible,
      enabled: record.enabled,
    });
    setFormOpen(true);
  };

  const handleDrop: TreeProps<MenuTreeNode>["onDrop"] = (info) => {
    const dragId = Number(info.dragNode.key);
    const dropId = Number(info.node.key);
    const dragged = findMenuLocation(menuTree, dragId)?.node;
    if (!dragged || dragId === dropId || collectMenuIds(dragged).has(dropId)) {
      return;
    }

    const removed = removeMenuNode(menuTree, dragId);
    if (!removed.removed) {
      return;
    }
    const target = findMenuLocation(removed.menus, dropId);
    if (!target) {
      return;
    }

    let nextTree: MenuTreeNode[];
    if (!info.dropToGap) {
      nextTree = insertMenuNode(
        removed.menus,
        target.node.id,
        target.node.children.length,
        removed.removed,
      );
    } else {
      const positionParts = info.node.pos.split("-");
      const relativePosition =
        info.dropPosition - Number(positionParts[positionParts.length - 1]);
      nextTree = insertMenuNode(
        removed.menus,
        target.parentId,
        target.index + (relativePosition > 0 ? 1 : 0),
        removed.removed,
      );
    }
    reorderMutation.mutate(nextTree);
  };

  return (
    <CrudPage
      title="菜单管理"
      subtitle="维护后台侧边栏、前端路由和菜单权限编码"
      breadcrumb={["系统管理", "菜单管理"]}
      icon={<MenuOutlined />}
      toolbar={
        <CrudToolbar
          actions={[
            {
              key: "create",
              label: "新增",
              icon: <PlusOutlined />,
              primary: true,
              permission: "system:menu:create",
              onClick: () => {
                setEditing(null);
                form.resetFields();
                form.setFieldsValue({
                  visible: true,
                  enabled: true,
                  sort_order: 0,
                });
                setFormOpen(true);
              },
            },
          ]}
        />
      }
    >
      <div className="menu-tree-workbench">
        <div className="menu-tree-hint">
          <Typography.Text type="secondary">
            {canReorder
              ? "拖动菜单可调整顺序；拖到菜单名称上可设为其子菜单。"
              : "当前账号仅可查看菜单层级。"}
          </Typography.Text>
          {reorderMutation.isPending ? (
            <Tag color="processing">保存中</Tag>
          ) : null}
        </div>
        <Tree<MenuTreeNode>
          blockNode
          defaultExpandAll
          draggable={canReorder && !reorderMutation.isPending}
          treeData={menuTree}
          onDrop={handleDrop}
          titleRender={(menu) => (
            <div className="menu-tree-node">
              <div className="menu-tree-node-main">
                <span className="menu-tree-node-icon">
                  {renderMenuIcon(menu.icon)}
                </span>
                <div className="menu-tree-node-copy">
                  <Typography.Text strong>{menu.title}</Typography.Text>
                  <div className="menu-tree-node-meta">
                    {menu.path ? <span>{menu.path}</span> : null}
                    {menu.permission_code ? (
                      <code>{menu.permission_code}</code>
                    ) : null}
                  </div>
                </div>
              </div>
              <div className="menu-tree-node-status">
                {!menu.visible ? <Tag>已隐藏</Tag> : null}
                {!menu.enabled ? <Tag color="error">已停用</Tag> : null}
              </div>
              <div className="menu-tree-node-actions">
                <PermissionButton
                  size="small"
                  icon={<EditOutlined />}
                  permission="system:menu:update"
                  aria-label={`编辑${menu.title}`}
                  title="编辑"
                  onClick={(event) => {
                    event.stopPropagation();
                    openEditor(menu);
                  }}
                >
                  编辑
                </PermissionButton>
                <PermissionButton
                  size="small"
                  danger
                  icon={<DeleteOutlined />}
                  permission="system:menu:delete"
                  loading={deleteMutation.isPending}
                  aria-label={`删除${menu.title}`}
                  title="删除"
                  onClick={(event) => {
                    event.stopPropagation();
                    Modal.confirm({
                      title: `删除“${menu.title}”？`,
                      content: "删除父菜单前，请确认其子菜单已完成迁移。",
                      okText: "删除",
                      okButtonProps: { danger: true },
                      cancelText: "取消",
                      onOk: () => deleteMutation.mutateAsync(menu.id),
                    });
                  }}
                >
                  删除
                </PermissionButton>
              </div>
            </div>
          )}
        />
      </div>
      <Modal
        title={editing ? "编辑菜单" : "新增菜单"}
        open={formOpen}
        onCancel={() => setFormOpen(false)}
        onOk={() => form.submit()}
        confirmLoading={saveMutation.isPending}
        width={720}
      >
        <Form
          form={form}
          layout="vertical"
          onFinish={(values) => saveMutation.mutate(values)}
        >
          <Form.Item name="parent_id" label="上级菜单">
            <Select
              allowClear
              options={flatMenus
                .filter((menu) => !editingMenuIds.has(menu.id))
                .map((menu) => ({ label: menu.title, value: menu.id }))}
            />
          </Form.Item>
          <Form.Item name="title" label="菜单名称" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item name="path" label="前端路径">
            <Input placeholder="/admin/system/users" />
          </Form.Item>
          <Form.Item name="icon" label="图标">
            <Select
              allowClear
              showSearch
              placeholder="请选择图标"
              optionFilterProp="label"
              options={iconSelectOptions(form.getFieldValue("icon")).map(
                (option) => ({
                  value: option.value,
                  label: option.label,
                }),
              )}
              optionRender={(option) => {
                const icon = iconSelectOptions(form.getFieldValue("icon")).find(
                  (item) => item.value === option.value,
                );
                return (
                  <Space size={8}>
                    {icon?.icon}
                    <span>{icon?.label ?? option.label}</span>
                    <span className="menu-icon-code">{option.value}</span>
                  </Space>
                );
              }}
              labelRender={(option) => (
                <Space size={8}>
                  {renderMenuIcon(String(option.value))}
                  <span>{option.label}</span>
                  <span className="menu-icon-code">{option.value}</span>
                </Space>
              )}
            />
          </Form.Item>
          <Form.Item name="permission_code" label="菜单权限编码">
            <Input placeholder="system:user:list" />
          </Form.Item>
          <Form.Item name="sort_order" label="排序">
            <InputNumber min={0} className="full-width" />
          </Form.Item>
          <Space>
            <Form.Item name="visible" label="显示" valuePropName="checked">
              <Switch />
            </Form.Item>
            <Form.Item name="enabled" label="启用" valuePropName="checked">
              <Switch />
            </Form.Item>
          </Space>
        </Form>
      </Modal>
    </CrudPage>
  );
}
