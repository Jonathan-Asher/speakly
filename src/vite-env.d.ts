/// <reference types="vite/client" />

declare module "*.ttf?inline" {
  const dataUrl: string;
  export default dataUrl;
}
