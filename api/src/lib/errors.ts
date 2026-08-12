/**
 * Error types the route handlers throw. app.ts's onError handler maps these
 * to the envelope shape from docs/architecture.md §4: 400 validation,
 * 404 not_found / not_in_coverage, 500 masked internals.
 */

export class HttpError extends Error {
  readonly status: number;
  readonly code: string;

  constructor(status: number, message: string, code: string) {
    super(message);
    this.name = "HttpError";
    this.status = status;
    this.code = code;
  }
}

export class ValidationError extends HttpError {
  constructor(message: string) {
    super(400, message, "validation_error");
    this.name = "ValidationError";
  }
}

export class NotFoundError extends HttpError {
  constructor(message = "not found") {
    super(404, message, "not_found");
    this.name = "NotFoundError";
  }
}

export class NotInCoverageError extends HttpError {
  constructor(message = "coordinate is outside Sri Lanka grid coverage") {
    super(404, message, "not_in_coverage");
    this.name = "NotInCoverageError";
  }
}

/** Formats a zod SafeParseError into a single readable message. */
export function formatZodIssues(issues: readonly { path: PropertyKey[]; message: string }[]): string {
  return issues
    .map((issue) => {
      const path = issue.path.join(".");
      return path ? `${path}: ${issue.message}` : issue.message;
    })
    .join("; ");
}
