import type Database from "better-sqlite3";

export interface StepContext {
  db: Database.Database;
  log: (msg: string) => void;
}

export interface Step {
  name: string;
  run(ctx: StepContext): Promise<void>;
}
