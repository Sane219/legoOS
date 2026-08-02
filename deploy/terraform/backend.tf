# Remote state backend.
#
# The S3 bucket and DynamoDB table below must exist BEFORE the first
# `terraform init` — Terraform can't create the place it stores its own
# state (the classic chicken-and-egg). Bootstrap them once, out of band,
# e.g.:
#
#   aws s3api create-bucket --bucket legoos-tfstate-<your-suffix> \
#     --region us-east-1
#   aws s3api put-bucket-versioning --bucket legoos-tfstate-<your-suffix> \
#     --versioning-configuration Status=Enabled
#   aws dynamodb create-table --table-name legoos-tfstate-lock \
#     --attribute-definitions AttributeName=LockID,AttributeType=S \
#     --key-schema AttributeName=LockID,KeyType=HASH \
#     --billing-mode PAY_PER_REQUEST
#
# Then fill in the bucket name below (bucket names are globally unique,
# so the placeholder will not work as-is) and run `terraform init`.
terraform {
  backend "s3" {
    bucket         = "legoos-tfstate-CHANGEME"
    key            = "legoos/terraform.tfstate"
    region         = "us-east-1"
    dynamodb_table = "legoos-tfstate-lock"
    encrypt        = true
  }
}
