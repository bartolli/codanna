/**
 * Comprehensive TypeScript test file for parser maturity assessment
 * Tests all major TypeScript language features and constructs
 */

// Imports and exports
import { readFile } from 'fs';
import * as path from 'path';
import type { RequestHandler } from 'express';
import { Component, OnInit } from '@angular/core';

// Re-exports
export { readFile } from 'fs';
export * from './types';
export type { RequestHandler };

// Module augmentation
declare module 'express' {
  interface Request {
    user?: User;
  }
}

// Global augmentation
declare global {
  interface Window {
    customAPI: any;
  }
}

// Namespaces
namespace Utils {
  export function formatDate(date: Date): string {
    return date.toISOString();
  }
  
  export namespace Math {
    export function add(a: number, b: number): number {
      return a + b;
    }
  }
}

// Type aliases
type ID = string | number;
type Nullable<T> = T | null;
type ReadonlyPartial<T> = Readonly<Partial<T>>;

// Complex type alias
type DeepPartial<T> = T extends object
  ? { [P in keyof T]?: DeepPartial<T[P]> }
  : T;

// Union types
type Status = 'pending' | 'processing' | 'completed' | 'failed';
type Result<T> = { success: true; data: T } | { success: false; error: Error };

// Intersection types
type Named = { name: string };
type Aged = { age: number };
type Person = Named & Aged & { email?: string };

// Conditional types
type IsArray<T> = T extends any[] ? true : false;
type ElementType<T> = T extends (infer E)[] ? E : T;

// Template literal types
type HTTPMethod = 'GET' | 'POST' | 'PUT' | 'DELETE';
type APIEndpoint = `/api/${string}`;
type RoutePattern = `${HTTPMethod} ${APIEndpoint}`;

// Mapped types
type Readonly<T> = {
  readonly [P in keyof T]: T[P];
};

type Optional<T> = {
  [P in keyof T]?: T[P];
};

// Key remapping
type Getters<T> = {
  [K in keyof T as `get${Capitalize<string & K>}`]: () => T[K];
};

// Interfaces
interface User {
  id: number;
  name: string;
  email: string;
  readonly createdAt: Date;
  settings?: UserSettings;
}

interface UserSettings {
  theme: 'light' | 'dark';
  notifications: boolean;
  [key: string]: any; // Index signature
}

// Interface extending
interface Admin extends User {
  role: 'admin';
  permissions: string[];
}

// Interface merging
interface Document {
  title: string;
}

interface Document {
  content: string;
}

// Generic interfaces
interface Container<T> {
  value: T;
  getValue(): T;
  setValue(value: T): void;
}

interface Map<K, V> {
  get(key: K): V | undefined;
  set(key: K, value: V): void;
  has(key: K): boolean;
  delete(key: K): boolean;
}

// Classes
class SimpleClass {
  // Properties
  public publicProp: string = 'public';
  private privateProp: string = 'private';
  protected protectedProp: string = 'protected';
  readonly readonlyProp: string = 'readonly';
  static staticProp: string = 'static';
  
  // Parameter properties
  constructor(
    public name: string,
    private id: number,
    protected data?: any
  ) {}
  
  // Methods
  public publicMethod(): void {
    console.log('Public method');
  }
  
  private privateMethod(): void {
    console.log('Private method');
  }
  
  protected protectedMethod(): void {
    console.log('Protected method');
  }
  
  static staticMethod(): void {
    console.log('Static method');
  }
  
  // Getters and setters
  get value(): string {
    return this.privateProp;
  }
  
  set value(val: string) {
    this.privateProp = val;
  }
  
  // Method overloading
  process(data: string): string;
  process(data: number): number;
  process(data: string | number): string | number {
    return data;
  }
}

// Abstract class
abstract class BaseService {
  abstract fetch<T>(url: string): Promise<T>;
  
  protected log(message: string): void {
    console.log(message);
  }
}

// Class inheritance
class UserService extends BaseService {
  async fetch<T>(url: string): Promise<T> {
    const response = await fetch(url);
    return response.json();
  }
}

// Class implementing interface
class UserImpl implements User {
  constructor(
    public id: number,
    public name: string,
    public email: string,
    public readonly createdAt: Date = new Date()
  ) {}
}

// Generic class
class GenericContainer<T> {
  private items: T[] = [];
  
  add(item: T): void {
    this.items.push(item);
  }
  
  get(index: number): T | undefined {
    return this.items[index];
  }
  
  map<U>(fn: (item: T) => U): U[] {
    return this.items.map(fn);
  }
}

// Class with decorators
@Component({
  selector: 'app-user',
  templateUrl: './user.component.html'
})
class UserComponent implements OnInit {
  @Input() user!: User;
  @Output() userChange = new EventEmitter<User>();
  
  ngOnInit(): void {
    console.log('Component initialized');
  }
  
  @HostListener('click', ['$event'])
  onClick(event: MouseEvent): void {
    console.log('Clicked', event);
  }
}

// Enums
enum Color {
  Red,
  Green,
  Blue
}

enum Direction {
  Up = 'UP',
  Down = 'DOWN',
  Left = 'LEFT',
  Right = 'RIGHT'
}

const enum FileAccess {
  None,
  Read = 1 << 1,
  Write = 1 << 2,
  ReadWrite = Read | Write
}

// Functions
function simpleFunction(x: number, y: number): number {
  return x + y;
}

// Function with optional and default parameters
function createUser(
  name: string,
  age?: number,
  active: boolean = true
): User {
  return {
    id: Date.now(),
    name,
    email: `${name}@example.com`,
    createdAt: new Date()
  };
}

// Rest parameters
function sum(...numbers: number[]): number {
  return numbers.reduce((a, b) => a + b, 0);
}

// Variable declaration with var (legacy style)
var legacyVariable = 'old-school';
var anotherVar: number = 42;

// Function expression
const myFunc = function(x: number): number {
  return x * 2;
};

const namedFuncExpr = function factorial(n: number): number {
  return n <= 1 ? 1 : n * factorial(n - 1);
};

// Function overloading
function parse(value: string): object;
function parse(value: string, reviver: (key: string, value: any) => any): object;
function parse(value: string, reviver?: (key: string, value: any) => any): object {
  return JSON.parse(value, reviver);
}

// Arrow functions
const add = (a: number, b: number): number => a + b;
const multiply = (a: number) => (b: number) => a * b;

// Generic functions
function identity<T>(value: T): T {
  return value;
}

function swap<T, U>(tuple: [T, U]): [U, T] {
  return [tuple[1], tuple[0]];
}

// Constrained generics
function getProperty<T, K extends keyof T>(obj: T, key: K): T[K] {
  return obj[key];
}

// Type guards
function isString(value: unknown): value is string {
  return typeof value === 'string';
}

function isUser(value: any): value is User {
  return value && typeof value.id === 'number' && typeof value.name === 'string';
}

// Type assertions
const someValue: unknown = 'Hello';
const strLength1 = (someValue as string).length;
const strLength2 = (someValue as string).length;

// Non-null assertion
function processUser(user?: User) {
  const name = user!.name; // Assert user is not null/undefined
}

// Async functions
async function fetchData<T>(url: string): Promise<T> {
  const response = await fetch(url);
  return response.json();
}

async function* asyncGenerator(): AsyncGenerator<number> {
  for (let i = 0; i < 10; i++) {
    yield await Promise.resolve(i);
  }
}

// Generators
function* fibonacci(): Generator<number> {
  let a = 0, b = 1;
  while (true) {
    yield a;
    [a, b] = [b, a + b];
  }
}

// Symbols
const sym1 = Symbol('key');
const sym2 = Symbol.for('shared');

interface SymbolIndexed {
  [sym1]: string;
  [Symbol.iterator](): Iterator<any>;
}

// Iterators and Iterables
class Range implements Iterable<number> {
  constructor(
    private start: number,
    private end: number
  ) {}
  
  *[Symbol.iterator](): Iterator<number> {
    for (let i = this.start; i <= this.end; i++) {
      yield i;
    }
  }
}

// Destructuring
const { name, age } = person;
const [first, second, ...rest] = array;

// Spread operator
const combined = { ...obj1, ...obj2 };
const concatenated = [...arr1, ...arr2];

// Object literals with computed properties
const propertyName = 'dynamicProp';
const obj = {
  [propertyName]: 'value',
  [`${propertyName}2`]: 'value2'
};

// this types
class FluentBuilder {
  setName(name: string): this {
    return this;
  }
  
  setAge(age: number): this {
    return this;
  }
  
  build(): User {
    return {} as User;
  }
}

// infer keyword
type ReturnType<T> = T extends (...args: any[]) => infer R ? R : never;
type UnpackPromise<T> = T extends Promise<infer U> ? U : T;

// Branded types
type UserId = number & { __brand: 'UserId' };
type PostId = number & { __brand: 'PostId' };

// Utility types usage
type PartialUser = Partial<User>;
type RequiredUser = Required<User>;
type ReadonlyUser = Readonly<User>;
type UserKeys = keyof User;
type UserName = User['name'];
type OmittedUser = Omit<User, 'id' | 'createdAt'>;
type PickedUser = Pick<User, 'name' | 'email'>;

// Module declaration
declare module '*.css' {
  const content: { [className: string]: string };
  export default content;
}

// Ambient declarations
declare const VERSION: string;
declare function require(module: string): any;

// Triple-slash directives
/// <reference types="node" />
/// <reference path="./types.d.ts" />

// Ambient module declaration
declare module "custom-module" {
  export const version: string;
  export function doSomething(): void;
}

// JSX/TSX
const element = <div className="container">Hello World</div>;
const selfClosing = <input type="text" />;

interface ButtonProps {
  onClick: () => void;
  children: React.ReactNode;
}

const Button: React.FC<ButtonProps> = ({ onClick, children }) => (
  <button onClick={onClick}>{children}</button>
);

// React-style component with hooks (arrow function in const)
/**
 * A themed component that manages theme state
 */
const ThemeCustomizer = () => {
  const theme = 'light';
  const count = 0;
  
  const handleClick = () => {
    console.log('Clicked');
    toggleTheme();
  };
  
  const toggleTheme = () => {
    console.log('Toggling theme');
  };
  
  return { theme, count, handleClick, toggleTheme };
};

// Helper function for theme component
function updateGlobalTheme(newTheme: string): void {
  console.log('Updating theme to:', newTheme);
}

// Another arrow function component pattern
const UserCard = (user: User) => {
  const formatDate = (date: Date) => date.toLocaleDateString();
  
  return {
    name: user.name,
    formattedDate: formatDate(user.createdAt)
  };
};

// Nested arrow functions in const declarations
const createHandler = (prefix: string) => {
  const innerHandler = (value: string) => {
    console.log(prefix + value);
  };
  return innerHandler;
};

// Variance annotations (4.7+)
interface Producer<out T> {
  produce(): T;
}

interface Consumer<in T> {
  consume(value: T): void;
}

// satisfies operator (4.9+)
const config = {
  host: 'localhost',
  port: 3000
} satisfies Record<string, string | number>;

// const type parameters (5.0+)
function constGeneric<const T>(value: T): T {
  return value;
}

// Export types
export type { User, Admin };
export interface Config {
  apiUrl: string;
  timeout: number;
}

// Default export
export default class Application {
  start(): void {
    console.log('Application started');
  }
}