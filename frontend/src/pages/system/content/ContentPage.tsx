import {
  BookOutlined,
  DeleteOutlined,
  EditOutlined,
  PlusOutlined,
  SendOutlined,
  StopOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  DatePicker,
  Form,
  Input,
  InputNumber,
  Modal,
  Popconfirm,
  Select,
  Space,
  Switch,
  Tabs,
  Tag,
  message,
} from "antd";
import type { ColumnsType } from "antd/es/table";
import dayjs from "dayjs";
import { useMemo, useState } from "react";
import {
  type ContentArticleRecord,
  type ContentCategoryRecord,
  type SaveContentArticleParams,
  type SaveContentCategoryParams,
  archiveContentArticle,
  createContentArticle,
  createContentCategory,
  deleteContentArticle,
  deleteContentCategory,
  fetchContentArticles,
  fetchContentCategories,
  publishContentArticle,
  updateContentArticle,
  updateContentCategory,
} from "../../../api/admin/content";
import { CrudPage } from "../../../components/admin/CrudPage";
import { CrudToolbar } from "../../../components/admin/CrudToolbar";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";

const statusOptions = [
  { value: "draft", label: "草稿" },
  { value: "published", label: "已发布" },
  { value: "archived", label: "已归档" },
];

const statusLabels: Record<string, { label: string; color: string }> = {
  draft: { label: "草稿", color: "default" },
  published: { label: "已发布", color: "green" },
  archived: { label: "已归档", color: "orange" },
};

type ArticleFormValues = Omit<SaveContentArticleParams, "published_at"> & {
  published_at?: dayjs.Dayjs | null;
};

export function ContentPage() {
  const [activeTab, setActiveTab] = useState("categories");

  return (
    <CrudPage
      title="内容管理"
      subtitle="维护后台模板的内容栏目、文章草稿、发布状态与 SEO 信息"
      breadcrumb={["系统管理", "内容管理"]}
      icon={<BookOutlined />}
    >
      <Tabs
        activeKey={activeTab}
        onChange={setActiveTab}
        items={[
          {
            key: "categories",
            label: "栏目管理",
            children: <ContentCategoriesTab />,
          },
          {
            key: "articles",
            label: "文章管理",
            children: <ContentArticlesTab />,
          },
        ]}
      />
    </CrudPage>
  );
}

function ContentCategoriesTab() {
  const [page, setPage] = useState(1);
  const [keyword, setKeyword] = useState("");
  const [enabled, setEnabled] = useState<boolean>();
  const [editing, setEditing] = useState<ContentCategoryRecord | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [form] = Form.useForm<SaveContentCategoryParams>();
  const queryClient = useQueryClient();

  const categoriesQuery = useQuery({
    queryKey: ["admin-content-categories", page, keyword, enabled],
    queryFn: () =>
      fetchContentCategories({
        page,
        page_size: 10,
        keyword: keyword || undefined,
        enabled,
      }),
  });

  const saveMutation = useMutation({
    mutationFn: (values: SaveContentCategoryParams) =>
      editing
        ? updateContentCategory(editing.id, values)
        : createContentCategory(values),
    onSuccess: () => {
      message.success("栏目已保存");
      setFormOpen(false);
      setEditing(null);
      queryClient.invalidateQueries({ queryKey: ["admin-content-categories"] });
    },
  });

  const deleteMutation = useMutation({
    mutationFn: deleteContentCategory,
    onSuccess: () => {
      message.success("栏目已删除");
      queryClient.invalidateQueries({ queryKey: ["admin-content-categories"] });
    },
  });

  const columns: ColumnsType<ContentCategoryRecord> = [
    { title: "名称", dataIndex: "name", width: 180 },
    { title: "Slug", dataIndex: "slug", width: 180 },
    {
      title: "启用",
      dataIndex: "enabled",
      width: 90,
      render: (value) => <Switch checked={value} disabled />,
    },
    { title: "排序", dataIndex: "sort_order", width: 90 },
    { title: "更新时间", dataIndex: "updated_at", width: 220 },
    {
      title: "操作",
      key: "actions",
      width: 180,
      render: (_, record) => (
        <Space wrap>
          <PermissionButton
            size="small"
            icon={<EditOutlined />}
            permission="system:content_category:update"
            onClick={() => {
              setEditing(record);
              form.setFieldsValue(record);
              setFormOpen(true);
            }}
          >
            编辑
          </PermissionButton>
          <Popconfirm
            title="确认删除栏目？"
            onConfirm={() => deleteMutation.mutate(record.id)}
          >
            <PermissionButton
              size="small"
              danger
              icon={<DeleteOutlined />}
              permission="system:content_category:delete"
            >
              删除
            </PermissionButton>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <Space direction="vertical" size="middle" className="full-width">
      <CrudToolbar
        actions={[
          {
            key: "create",
            label: "新增栏目",
            icon: <PlusOutlined />,
            primary: true,
            permission: "system:content_category:create",
            onClick: () => {
              setEditing(null);
              form.resetFields();
              form.setFieldsValue({ enabled: true, sort_order: 0 });
              setFormOpen(true);
            },
          },
        ]}
      />
      <Space wrap>
        <Input.Search
          allowClear
          placeholder="搜索栏目名称、Slug"
          onSearch={(value) => {
            setPage(1);
            setKeyword(value);
          }}
          className="admin-search-input"
        />
        <Select
          allowClear
          placeholder="启用状态"
          value={enabled}
          onChange={(value) => {
            setPage(1);
            setEnabled(value);
          }}
          options={[
            { value: true, label: "启用" },
            { value: false, label: "停用" },
          ]}
          className="admin-filter-select"
        />
      </Space>
      <DataTable<ContentCategoryRecord>
        columns={columns}
        dataSource={categoriesQuery.data?.items ?? []}
        loading={categoriesQuery.isLoading}
        pagination={{
          current: page,
          total: categoriesQuery.data?.total ?? 0,
          onChange: setPage,
        }}
      />

      <Modal
        title={editing ? "编辑栏目" : "新增栏目"}
        open={formOpen}
        onCancel={() => setFormOpen(false)}
        onOk={() => form.submit()}
        confirmLoading={saveMutation.isPending}
        width={640}
        destroyOnHidden
      >
        <Form
          form={form}
          layout="vertical"
          onFinish={(values) => saveMutation.mutate(values)}
        >
          <Form.Item name="name" label="栏目名称" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item
            name="slug"
            label="Slug"
            rules={[
              { required: true },
              {
                pattern: /^[a-z0-9-]+$/,
                message: "仅支持小写字母、数字和短横线",
              },
            ]}
          >
            <Input placeholder="product-news" />
          </Form.Item>
          <Form.Item name="description" label="描述">
            <Input.TextArea rows={3} />
          </Form.Item>
          <Form.Item name="sort_order" label="排序">
            <InputNumber className="full-width" />
          </Form.Item>
          <Form.Item name="enabled" label="启用" valuePropName="checked">
            <Switch />
          </Form.Item>
        </Form>
      </Modal>
    </Space>
  );
}

function ContentArticlesTab() {
  const [page, setPage] = useState(1);
  const [keyword, setKeyword] = useState("");
  const [categoryId, setCategoryId] = useState<number>();
  const [status, setStatus] = useState<string>();
  const [isFeatured, setIsFeatured] = useState<boolean>();
  const [editing, setEditing] = useState<ContentArticleRecord | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [form] = Form.useForm<ArticleFormValues>();
  const queryClient = useQueryClient();

  const categoriesQuery = useQuery({
    queryKey: ["admin-content-categories", "all"],
    queryFn: () => fetchContentCategories({ page: 1, page_size: 100 }),
  });

  const categoryOptions = useMemo(
    () =>
      (categoriesQuery.data?.items ?? []).map((item) => ({
        value: item.id,
        label: item.name,
      })),
    [categoriesQuery.data?.items],
  );

  const articlesQuery = useQuery({
    queryKey: [
      "admin-content-articles",
      page,
      keyword,
      categoryId,
      status,
      isFeatured,
    ],
    queryFn: () =>
      fetchContentArticles({
        page,
        page_size: 10,
        keyword: keyword || undefined,
        category_id: categoryId,
        status,
        is_featured: isFeatured,
      }),
  });

  const saveMutation = useMutation({
    mutationFn: (values: ArticleFormValues) => {
      const payload: SaveContentArticleParams = {
        ...values,
        published_at: values.published_at?.toISOString() ?? null,
      };
      return editing
        ? updateContentArticle(editing.id, payload)
        : createContentArticle(payload);
    },
    onSuccess: () => {
      message.success("文章已保存");
      setFormOpen(false);
      setEditing(null);
      queryClient.invalidateQueries({ queryKey: ["admin-content-articles"] });
    },
  });

  const publishMutation = useMutation({
    mutationFn: publishContentArticle,
    onSuccess: () => {
      message.success("文章已发布");
      queryClient.invalidateQueries({ queryKey: ["admin-content-articles"] });
    },
  });

  const archiveMutation = useMutation({
    mutationFn: archiveContentArticle,
    onSuccess: () => {
      message.success("文章已归档");
      queryClient.invalidateQueries({ queryKey: ["admin-content-articles"] });
    },
  });

  const deleteMutation = useMutation({
    mutationFn: deleteContentArticle,
    onSuccess: () => {
      message.success("文章已删除");
      queryClient.invalidateQueries({ queryKey: ["admin-content-articles"] });
    },
  });

  const columns: ColumnsType<ContentArticleRecord> = [
    { title: "标题", dataIndex: "title", width: 220, ellipsis: true },
    {
      title: "栏目",
      dataIndex: "category_id",
      width: 150,
      render: (value) =>
        categoryOptions.find((item) => item.value === value)?.label ?? value,
    },
    { title: "Slug", dataIndex: "slug", width: 180 },
    {
      title: "状态",
      dataIndex: "status",
      width: 100,
      render: (value) => {
        const status = statusLabels[value] ?? {
          label: value,
          color: "default",
        };
        return <Tag color={status.color}>{status.label}</Tag>;
      },
    },
    {
      title: "置顶",
      dataIndex: "is_featured",
      width: 90,
      render: (value) => <Switch checked={value} disabled />,
    },
    { title: "发布时间", dataIndex: "published_at", width: 220 },
    { title: "更新时间", dataIndex: "updated_at", width: 220 },
    {
      title: "操作",
      key: "actions",
      width: 300,
      render: (_, record) => (
        <Space wrap>
          <PermissionButton
            size="small"
            icon={<EditOutlined />}
            permission="system:content_article:update"
            onClick={() => {
              setEditing(record);
              form.setFieldsValue({
                ...record,
                published_at: record.published_at
                  ? dayjs(record.published_at)
                  : null,
              });
              setFormOpen(true);
            }}
          >
            编辑
          </PermissionButton>
          <PermissionButton
            size="small"
            icon={<SendOutlined />}
            permission="system:content_article:publish"
            disabled={record.status === "published"}
            onClick={() => publishMutation.mutate(record.id)}
          >
            发布
          </PermissionButton>
          <PermissionButton
            size="small"
            icon={<StopOutlined />}
            permission="system:content_article:update"
            disabled={record.status === "archived"}
            onClick={() => archiveMutation.mutate(record.id)}
          >
            归档
          </PermissionButton>
          <Popconfirm
            title="确认删除文章？"
            onConfirm={() => deleteMutation.mutate(record.id)}
          >
            <PermissionButton
              size="small"
              danger
              icon={<DeleteOutlined />}
              permission="system:content_article:delete"
            >
              删除
            </PermissionButton>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <Space direction="vertical" size="middle" className="full-width">
      <CrudToolbar
        actions={[
          {
            key: "create",
            label: "新增文章",
            icon: <PlusOutlined />,
            primary: true,
            permission: "system:content_article:create",
            onClick: () => {
              setEditing(null);
              form.resetFields();
              form.setFieldsValue({
                status: "draft",
                is_featured: false,
                category_id: categoryOptions[0]?.value,
              });
              setFormOpen(true);
            },
          },
        ]}
      />
      <Space wrap>
        <Input.Search
          allowClear
          placeholder="搜索标题、Slug"
          onSearch={(value) => {
            setPage(1);
            setKeyword(value);
          }}
          className="admin-search-input"
        />
        <Select
          allowClear
          placeholder="栏目"
          value={categoryId}
          onChange={(value) => {
            setPage(1);
            setCategoryId(value);
          }}
          options={categoryOptions}
          className="admin-filter-select"
        />
        <Select
          allowClear
          placeholder="状态"
          value={status}
          onChange={(value) => {
            setPage(1);
            setStatus(value);
          }}
          options={statusOptions}
          className="admin-filter-select"
        />
        <Select
          allowClear
          placeholder="置顶"
          value={isFeatured}
          onChange={(value) => {
            setPage(1);
            setIsFeatured(value);
          }}
          options={[
            { value: true, label: "置顶" },
            { value: false, label: "普通" },
          ]}
          className="admin-filter-select"
        />
      </Space>
      <DataTable<ContentArticleRecord>
        columns={columns}
        dataSource={articlesQuery.data?.items ?? []}
        loading={articlesQuery.isLoading || categoriesQuery.isLoading}
        pagination={{
          current: page,
          total: articlesQuery.data?.total ?? 0,
          onChange: setPage,
        }}
      />

      <Modal
        title={editing ? "编辑文章" : "新增文章"}
        open={formOpen}
        onCancel={() => setFormOpen(false)}
        onOk={() => form.submit()}
        confirmLoading={saveMutation.isPending}
        width={900}
        destroyOnHidden
      >
        <Form
          form={form}
          layout="vertical"
          onFinish={(values) => saveMutation.mutate(values)}
        >
          <Form.Item
            name="category_id"
            label="栏目"
            rules={[{ required: true }]}
          >
            <Select options={categoryOptions} />
          </Form.Item>
          <Form.Item name="title" label="标题" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item
            name="slug"
            label="Slug"
            rules={[
              { required: true },
              {
                pattern: /^[a-z0-9-]+$/,
                message: "仅支持小写字母、数字和短横线",
              },
            ]}
          >
            <Input placeholder="release-notes" />
          </Form.Item>
          <Form.Item name="summary" label="摘要">
            <Input.TextArea rows={3} />
          </Form.Item>
          <Form.Item name="content" label="正文" rules={[{ required: true }]}>
            <Input.TextArea rows={10} />
          </Form.Item>
          <Form.Item name="cover_image_url" label="封面 URL">
            <Input />
          </Form.Item>
          <Space wrap>
            <Form.Item name="status" label="状态" rules={[{ required: true }]}>
              <Select options={statusOptions} className="admin-filter-select" />
            </Form.Item>
            <Form.Item name="published_at" label="发布时间">
              <DatePicker showTime className="admin-filter-select" />
            </Form.Item>
            <Form.Item name="is_featured" label="置顶" valuePropName="checked">
              <Switch />
            </Form.Item>
          </Space>
          <Form.Item name="seo_title" label="SEO 标题">
            <Input />
          </Form.Item>
          <Form.Item name="seo_description" label="SEO 描述">
            <Input.TextArea rows={3} />
          </Form.Item>
        </Form>
      </Modal>
    </Space>
  );
}
