import { useState } from 'react';
import { AuthProvider, useAuth } from '@context/AuthContext';
import { Button } from '@components/Button';
import { Input } from '@components/Input';
import { Modal, ConfirmModal } from '@components/Modal';
import { useDebounce } from '@hooks/useDebounce';
import { useMutation } from '@hooks/useApi';
import { authService } from '@services/auth';
import { validateEmail, validatePassword } from '@utils/validation';

function LoginForm() {
  const { login, loading, error } = useAuth();
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [formErrors, setFormErrors] = useState({});

  const debouncedEmail = useDebounce(email, 300);

  const handleSubmit = async (e) => {
    e.preventDefault();

    const errors = {};
    if (!validateEmail(email)) {
      errors.email = 'Please enter a valid email';
    }

    const passwordValidation = validatePassword(password);
    if (!passwordValidation.valid) {
      errors.password = passwordValidation.errors[0];
    }

    if (Object.keys(errors).length > 0) {
      setFormErrors(errors);
      return;
    }

    setFormErrors({});
    await login({ email, password });
  };

  return (
    <form onSubmit={handleSubmit} className="space-y-4 max-w-md mx-auto p-6">
      <h1 className="text-2xl font-bold text-center">Login</h1>

      {error && (
        <div className="p-3 bg-red-100 text-red-700 rounded-md">{error}</div>
      )}

      <Input
        label="Email"
        type="email"
        value={email}
        onChange={(e) => setEmail(e.target.value)}
        error={formErrors.email}
        required
        fullWidth
      />

      <Input
        label="Password"
        type="password"
        value={password}
        onChange={(e) => setPassword(e.target.value)}
        error={formErrors.password}
        required
        fullWidth
      />

      <Button type="submit" loading={loading} fullWidth>
        Sign In
      </Button>
    </form>
  );
}

function Dashboard() {
  const { user, logout } = useAuth();
  const [showLogoutConfirm, setShowLogoutConfirm] = useState(false);

  return (
    <div className="p-6">
      <div className="flex justify-between items-center mb-6">
        <h1 className="text-2xl font-bold">Welcome, {user?.name}</h1>
        <Button variant="ghost" onClick={() => setShowLogoutConfirm(true)}>
          Logout
        </Button>
      </div>

      <ConfirmModal
        isOpen={showLogoutConfirm}
        onClose={() => setShowLogoutConfirm(false)}
        onConfirm={logout}
        title="Confirm Logout"
        message="Are you sure you want to log out?"
        confirmText="Logout"
      />
    </div>
  );
}

function AppContent() {
  const { isAuthenticated } = useAuth();

  return isAuthenticated ? <Dashboard /> : <LoginForm />;
}

export default function App() {
  return (
    <AuthProvider>
      <AppContent />
    </AuthProvider>
  );
}
