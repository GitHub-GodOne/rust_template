import { create } from "zustand";

export type RunningUpload = {
  file: File;
  paused: boolean;
};

type UploadTasksState = {
  runningUploads: Record<number, RunningUpload>;
  setTaskRunning: (taskId: number, upload?: RunningUpload) => void;
  pauseTask: (taskId: number) => void;
};

export const useUploadTasksStore = create<UploadTasksState>((set) => ({
  runningUploads: {},
  setTaskRunning: (taskId, upload) =>
    set((state) => {
      const next = { ...state.runningUploads };
      if (upload) {
        next[taskId] = upload;
      } else {
        delete next[taskId];
      }
      return { runningUploads: next };
    }),
  pauseTask: (taskId) =>
    set((state) => {
      const running = state.runningUploads[taskId];
      if (!running) {
        return state;
      }
      return {
        runningUploads: {
          ...state.runningUploads,
          [taskId]: { ...running, paused: true },
        },
      };
    }),
}));
