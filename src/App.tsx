import { HashRouter, Route, Routes } from "react-router";
import ProjectWizard from "./components/ProjectWizard";
import Home from "./pages/Home";
import Project from "./pages/Project";

// 用 HashRouter：打包后的页面从本地文件协议加载，没有服务器来处理路径路由。
export default function App() {
  return (
    <HashRouter>
      <Routes>
        <Route path="/" element={<Home />} />
        <Route path="/new" element={<ProjectWizard />} />
        <Route path="/project/:id" element={<Project />} />
      </Routes>
    </HashRouter>
  );
}
