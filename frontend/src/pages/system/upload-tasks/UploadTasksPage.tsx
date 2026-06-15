import { CloudSyncOutlined, ReloadOutlined } from "@ant-design/icons";
import { useQuery } from "@tanstack/react-query";
import { Alert, Button, Card, Space, Typography } from "antd";
import { fetchUploadTasks } from "../../../api/admin/uploads";
import { CrudPage } from "../../../components/admin/CrudPage";
import { DataTable } from "../../../components/admin/DataTable";
import {
  createUploadTaskColumns,
  useUploadTaskManager,
} from "../uploads/uploadTaskManager";

export function UploadTasksPage() {
  const uploadTasksQuery = useQuery({
    queryKey: ["admin-upload-tasks"],
    queryFn: fetchUploadTasks,
    refetchInterval: 5000,
  });
  const taskManager = useUploadTaskManager({ visibility: "private" });
  const columns = createUploadTaskColumns(taskManager);
  const uploadTasks = uploadTasksQuery.data ?? [];

  return (
    <CrudPage
      title="上传任务"
      subtitle="管理分片上传、续传与入库状态"
      breadcrumb={["系统管理", "上传任务"]}
      icon={<CloudSyncOutlined />}
      toolbar={
        <Button
          icon={<ReloadOutlined />}
          loading={uploadTasksQuery.isFetching}
          onClick={() => uploadTasksQuery.refetch()}
        >
          刷新
        </Button>
      }
    >
      <Space direction="vertical" size="middle" className="full-width">
        <Alert
          type="info"
          showIcon
          message="刷新页面后，浏览器无法自动恢复本地文件对象；需要续传时请重新选择同一个文件。"
        />
        <Card className="admin-card upload-task-page-card">
          <Space direction="vertical" size="middle" className="full-width">
            <Typography.Text type="secondary">
              当前共 {uploadTasks.length} 个上传任务，入库中任务会自动刷新状态。
            </Typography.Text>
            <DataTable
              columns={columns}
              dataSource={uploadTasks}
              loading={uploadTasksQuery.isLoading}
              pagination={false}
              scroll={{ x: 1100 }}
            />
          </Space>
        </Card>
      </Space>
    </CrudPage>
  );
}
