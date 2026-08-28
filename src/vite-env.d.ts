/// <reference types="vite/client" />

/** 构建时间戳，由 vite.config.ts 注入 —— 用来回答「我跑的是哪个构建」 */
declare const __BUILD_TIME__: string;
