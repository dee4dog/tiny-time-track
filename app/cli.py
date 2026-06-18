"""Command-line admin tools.

Run from the project root with the virtualenv active. Examples:

    # Create the database tables (also happens automatically on first run)
    python -m app.cli init-db

    # Create the first manager account (prompts for a password)
    python -m app.cli create-manager --name "Dirk" --email dirk@es.co.za

    # Add a normal employee
    python -m app.cli create-employee --name "Anel" --email anel@es.co.za --salary 480000

    # Reset someone's password
    python -m app.cli set-password --email dirk@es.co.za

    # Load demo projects + sample staff (safe to run once on an empty db)
    python -m app.cli seed
"""
from __future__ import annotations

import argparse
import getpass
import sys
from datetime import date
from decimal import Decimal, InvalidOperation

from sqlalchemy.orm import Session

from app.database import SessionLocal, create_all_tables
from app.models import Employee, RateHistory, Role
from app.security import hash_password


def _prompt_password() -> str:
    """Ask for a password twice (hidden) and confirm they match."""
    while True:
        pw1 = getpass.getpass("Password: ")
        if len(pw1) < 8:
            print("  Password must be at least 8 characters. Try again.")
            continue
        pw2 = getpass.getpass("Confirm password: ")
        if pw1 != pw2:
            print("  Passwords did not match. Try again.")
            continue
        return pw1


def _create_user(
    db: Session, *, name: str, email: str, role: Role, salary: Decimal | None
) -> Employee:
    """Insert an employee, plus an initial rate_history row when salary given."""
    email = email.strip().lower()
    existing = db.query(Employee).filter(Employee.email == email).one_or_none()
    if existing is not None:
        raise SystemExit(f"An account with email {email!r} already exists.")

    password = _prompt_password()
    employee = Employee(
        name=name.strip(),
        email=email,
        role=role,
        annual_salary=salary or Decimal("0"),
        password_hash=hash_password(password),
        active=True,
    )
    db.add(employee)
    db.flush()  # assign employee.id

    if salary is not None:
        db.add(
            RateHistory(
                employee_id=employee.id,
                effective_date=date.today(),
                annual_salary=salary,
            )
        )
    db.commit()
    return employee


def _parse_salary(raw: str | None) -> Decimal | None:
    if raw is None:
        return None
    try:
        return Decimal(raw)
    except InvalidOperation:
        raise SystemExit(f"Invalid salary value: {raw!r}")


def cmd_init_db(_args: argparse.Namespace) -> None:
    create_all_tables()
    print("Database tables are ready.")


def cmd_create_manager(args: argparse.Namespace) -> None:
    create_all_tables()
    with SessionLocal() as db:
        user = _create_user(
            db,
            name=args.name,
            email=args.email,
            role=Role.manager,
            salary=_parse_salary(args.salary),
        )
    print(f"Created manager: {user.name} <{user.email}> (id={user.id})")


def cmd_create_employee(args: argparse.Namespace) -> None:
    create_all_tables()
    with SessionLocal() as db:
        user = _create_user(
            db,
            name=args.name,
            email=args.email,
            role=Role.employee,
            salary=_parse_salary(args.salary),
        )
    print(f"Created employee: {user.name} <{user.email}> (id={user.id})")


def cmd_set_password(args: argparse.Namespace) -> None:
    with SessionLocal() as db:
        email = args.email.strip().lower()
        user = db.query(Employee).filter(Employee.email == email).one_or_none()
        if user is None:
            raise SystemExit(f"No account found for {email!r}.")
        user.password_hash = hash_password(_prompt_password())
        db.commit()
    print(f"Password updated for {email}.")


def cmd_seed(_args: argparse.Namespace) -> None:
    from app.seed import seed_demo_data

    create_all_tables()
    with SessionLocal() as db:
        seed_demo_data(db)
    print("Demo data loaded.")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="python -m app.cli", description="ES TimeTrack admin CLI")
    sub = parser.add_subparsers(dest="command", required=True)

    p = sub.add_parser("init-db", help="create database tables")
    p.set_defaults(func=cmd_init_db)

    p = sub.add_parser("create-manager", help="create a manager account")
    p.add_argument("--name", required=True)
    p.add_argument("--email", required=True)
    p.add_argument("--salary", help="annual salary in ZAR (optional)")
    p.set_defaults(func=cmd_create_manager)

    p = sub.add_parser("create-employee", help="create an employee account")
    p.add_argument("--name", required=True)
    p.add_argument("--email", required=True)
    p.add_argument("--salary", help="annual salary in ZAR (optional)")
    p.set_defaults(func=cmd_create_employee)

    p = sub.add_parser("set-password", help="reset an account's password")
    p.add_argument("--email", required=True)
    p.set_defaults(func=cmd_set_password)

    p = sub.add_parser("seed", help="load demo projects and sample staff")
    p.set_defaults(func=cmd_seed)

    return parser


def main(argv: list[str] | None = None) -> None:
    args = build_parser().parse_args(argv)
    args.func(args)


if __name__ == "__main__":
    main(sys.argv[1:])
