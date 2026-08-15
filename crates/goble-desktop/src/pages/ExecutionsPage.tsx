import { useEffect } from 'react';
import { useNavigate } from 'react-router-dom';

export default function ExecutionsPage() {
  const navigate = useNavigate();
  useEffect(() => {
    navigate('/traces', { replace: true });
  }, [navigate]);
  return null;
}
