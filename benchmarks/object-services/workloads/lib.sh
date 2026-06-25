require_positive_integer() {
  name="$1"
  value="$2"

  case "$value" in
    ''|*[!0-9]*)
      echo "$name must be a positive integer" >&2
      exit 64
      ;;
  esac

  if [ "$value" -eq 0 ]; then
    echo "$name must be greater than zero" >&2
    exit 64
  fi
}
