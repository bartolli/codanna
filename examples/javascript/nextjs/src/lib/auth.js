import { cookies } from 'next/headers';
import { findUserById, findUserByEmail } from '@/lib/db';

const SESSION_COOKIE = 'session_id';
const SESSION_MAX_AGE = 60 * 60 * 24 * 7; // 7 days

// In-memory session store (use Redis in production)
const sessions = new Map();

export async function createSession(userId) {
  const sessionId = crypto.randomUUID();
  const expiresAt = new Date(Date.now() + SESSION_MAX_AGE * 1000);

  sessions.set(sessionId, {
    userId,
    expiresAt,
  });

  cookies().set(SESSION_COOKIE, sessionId, {
    httpOnly: true,
    secure: process.env.NODE_ENV === 'production',
    sameSite: 'lax',
    maxAge: SESSION_MAX_AGE,
    path: '/',
  });

  return sessionId;
}

export async function getSession() {
  const sessionId = cookies().get(SESSION_COOKIE)?.value;
  if (!sessionId) {
    return null;
  }

  const session = sessions.get(sessionId);
  if (!session) {
    return null;
  }

  if (new Date() > session.expiresAt) {
    sessions.delete(sessionId);
    return null;
  }

  return session;
}

export async function getCurrentUser() {
  const session = await getSession();
  if (!session) {
    return null;
  }

  return findUserById(session.userId);
}

export async function destroySession() {
  const sessionId = cookies().get(SESSION_COOKIE)?.value;
  if (sessionId) {
    sessions.delete(sessionId);
    cookies().delete(SESSION_COOKIE);
  }
}

export async function authenticate(email, password) {
  // In production, verify password hash
  const user = await findUserByEmail(email);
  if (!user) {
    throw new Error('Invalid credentials');
  }

  await createSession(user.id);
  return user;
}

export function requireAuth(handler) {
  return async function (...args) {
    const user = await getCurrentUser();
    if (!user) {
      throw new Error('Unauthorized');
    }
    return handler(...args, user);
  };
}

export function requireRole(roles, handler) {
  return requireAuth(async function (...args) {
    const user = args[args.length - 1];
    if (!roles.includes(user.role)) {
      throw new Error('Forbidden');
    }
    return handler(...args);
  });
}
