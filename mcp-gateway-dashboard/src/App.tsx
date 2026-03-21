import { Routes, Route, Navigate } from 'react-router-dom';
import { useAuth } from '@/hooks/useAuth';
import Layout from '@/components/Layout';
import Login from '@/pages/Login';
import ToolInventory from '@/pages/ToolInventory';
import AuditTimeline from '@/pages/AuditTimeline';
import MetricsOverview from '@/pages/MetricsOverview';
import UserManagement from '@/pages/UserManagement';
import PolicyEditor from '@/pages/PolicyEditor';
import BackendConfig from '@/pages/BackendConfig';
import Settings from '@/pages/Settings';
import UsageGraph from '@/pages/UsageGraph';

export default function App() {
  const auth = useAuth();

  if (!auth.user) {
    return (
      <Routes>
        <Route path="/login" element={<Login auth={auth} />} />
        <Route path="*" element={<Navigate to="/login" replace />} />
      </Routes>
    );
  }

  // Non-owner users only see the backends/configure page
  if (!auth.isAdmin) {
    return (
      <Layout auth={auth}>
        <Routes>
          <Route path="/" element={<Navigate to="/backends" replace />} />
          <Route path="/backends" element={<BackendConfig isAdmin={auth.isAdmin} />} />
          <Route path="/usage" element={<UsageGraph isAdmin={auth.isAdmin} />} />
          <Route path="*" element={<Navigate to="/backends" replace />} />
        </Routes>
      </Layout>
    );
  }

  return (
    <Layout auth={auth}>
      <Routes>
        <Route path="/" element={<Navigate to="/tools" replace />} />
        <Route path="/tools" element={<ToolInventory />} />
        <Route path="/audit" element={<AuditTimeline />} />
        <Route path="/metrics" element={<MetricsOverview />} />
        <Route path="/users" element={<UserManagement />} />
        <Route path="/policies" element={<PolicyEditor />} />
        <Route path="/backends" element={<BackendConfig isAdmin={auth.isAdmin} />} />
        <Route path="/usage" element={<UsageGraph isAdmin={auth.isAdmin} />} />
        <Route path="/settings" element={<Settings />} />
        <Route path="*" element={<Navigate to="/tools" replace />} />
      </Routes>
    </Layout>
  );
}
